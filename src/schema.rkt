#lang racket/base

(require racket/list
         racket/string
         "ast.rkt")

(provide (struct-out schema-validation)
         validate-schema)

(struct schema-validation (valid? value issues) #:transparent)

(define (validate-schema specification value
                         #:step [step void]
                         #:max-issues [maximum-issues 32])
  (unless (procedure? step)
    (raise-argument-error 'validate-schema "procedure?" step))
  (unless (and (exact-integer? maximum-issues) (positive? maximum-issues))
    (raise-argument-error 'validate-schema
                          "exact-positive-integer?"
                          maximum-issues))
  (define reversed-issues '())
  (define issue-count 0)
  (define truncated? #f)

  (define (add-issue! path code message . details)
    (if (< issue-count maximum-issues)
        (begin
          (set! reversed-issues
                (cons
                 (for/fold ([issue
                             (hash "path" (path->json-pointer path)
                                   "code" code
                                   "message" message)])
                           ([entry (in-list details)])
                   (hash-set issue (car entry) (cdr entry)))
                 reversed-issues))
          (set! issue-count (add1 issue-count)))
        (set! truncated? #t)))

  (define (visit current value path)
    (step)
    (cond
      [(schema-any? current) value]
      [(schema-string? current)
       (cond
         [(not (string? value))
          (add-issue! path
                      "SCHEMA_TYPE"
                      "expected a string"
                      (cons "expected" "string")
                      (cons "actual" (schema-value-kind value)))
          value]
         [else
          (define size (string-length value))
          (when (< size (schema-string-minimum-length current))
            (add-issue! path
                        "SCHEMA_MIN_LENGTH"
                        "string is shorter than the minimum length"
                        (cons "minimum" (schema-string-minimum-length current))
                        (cons "actual" size)))
          (when (and (schema-string-maximum-length current)
                     (> size (schema-string-maximum-length current)))
            (add-issue! path
                        "SCHEMA_MAX_LENGTH"
                        "string is longer than the maximum length"
                        (cons "maximum" (schema-string-maximum-length current))
                        (cons "actual" size)))
          value])]
      [(schema-integer? current)
       (cond
         [(not (exact-integer? value))
          (add-issue! path
                      "SCHEMA_TYPE"
                      "expected an integer"
                      (cons "expected" "integer")
                      (cons "actual" (schema-value-kind value)))
          value]
         [else
          (when (and (schema-integer-minimum current)
                     (< value (schema-integer-minimum current)))
            (add-issue! path
                        "SCHEMA_MINIMUM"
                        "integer is below the minimum"
                        (cons "minimum" (schema-integer-minimum current))
                        (cons "actual" value)))
          (when (and (schema-integer-maximum current)
                     (> value (schema-integer-maximum current)))
            (add-issue! path
                        "SCHEMA_MAXIMUM"
                        "integer is above the maximum"
                        (cons "maximum" (schema-integer-maximum current))
                        (cons "actual" value)))
          value])]
      [(schema-boolean? current)
       (unless (boolean? value)
         (add-issue! path
                     "SCHEMA_TYPE"
                     "expected a boolean"
                     (cons "expected" "boolean")
                     (cons "actual" (schema-value-kind value))))
       value]
      [(schema-list? current)
       (cond
         [(not (list? value))
          (add-issue! path
                      "SCHEMA_TYPE"
                      "expected a list"
                      (cons "expected" "list")
                      (cons "actual" (schema-value-kind value)))
          value]
         [else
          (define size (length value))
          (when (< size (schema-list-minimum-length current))
            (add-issue! path
                        "SCHEMA_MIN_LENGTH"
                        "list is shorter than the minimum length"
                        (cons "minimum" (schema-list-minimum-length current))
                        (cons "actual" size)))
          (when (> size (schema-list-maximum-length current))
            (add-issue! path
                        "SCHEMA_MAX_LENGTH"
                        "list is longer than the maximum length"
                        (cons "maximum" (schema-list-maximum-length current))
                        (cons "actual" size)))
          (for/list ([item (in-list value)] [index (in-naturals)])
            (visit (schema-list-item current)
                   item
                   (append path (list (number->string index)))))])]
      [(schema-object? current)
       (cond
         [(not (hash? value))
          (add-issue! path
                      "SCHEMA_TYPE"
                      "expected an object"
                      (cons "expected" "object")
                      (cons "actual" (schema-value-kind value)))
          value]
         [else
          (define normalized (hash))
          (define declared-names
            (map schema-field-name (schema-object-fields current)))
          (for ([field (in-list (schema-object-fields current))])
            (define name (schema-field-name field))
            (cond
              [(hash-has-key? value name)
               (set! normalized
                     (hash-set normalized
                               name
                               (visit (schema-field-specification field)
                                      (hash-ref value name)
                                      (append path (list name)))))]
              [(schema-field-has-default? field)
               (set! normalized
                     (hash-set normalized name (schema-field-default field)))]
              [(schema-field-required? field)
               (add-issue! (append path (list name))
                           "SCHEMA_REQUIRED"
                           "required field is missing")]))
          (for ([key (in-list (sort (hash-keys value) key<?))]
                #:unless (and (string? key) (member key declared-names)))
            (add-issue! (append path (list (format "~a" key)))
                        "SCHEMA_ADDITIONAL_PROPERTY"
                        "field is not declared by the schema"))
          normalized])]
      [else
       (error 'validate-schema "unknown schema node: ~s" current)]))

  (define normalized (visit specification value '()))
  (define issues (reverse reversed-issues))
  (define final-issues
    (if truncated?
        (append
         (take issues (max 0 (sub1 maximum-issues)))
         (list
          (hash "path" ""
                "code" "SCHEMA_ISSUES_TRUNCATED"
                "message" "additional validation issues were omitted")))
        issues))
  (schema-validation (null? final-issues) normalized final-issues))

(define (path->json-pointer path)
  (if (null? path)
      ""
      (string-append
       "/"
       (string-join (map escape-pointer-token path) "/"))))

(define (escape-pointer-token value)
  (string-replace (string-replace value "~" "~0") "/" "~1"))

(define (key<? left right)
  (string<? (format "~a" left) (format "~a" right)))

(define (schema-value-kind value)
  (cond
    [(null? value) "null-or-empty-list"]
    [(boolean? value) "boolean"]
    [(exact-integer? value) "integer"]
    [(string? value) "string"]
    [(list? value) "list"]
    [(hash? value) "object"]
    [else "unsupported"]))
