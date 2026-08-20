#lang racket/base

(require json
         racket/file
         racket/list
         racket/path
         "ast.rkt"
         "error.rkt"
         "runtime.rkt")

(provide load-conformance-manifest
         run-conformance-manifest
         fixture->value
         value->fixture)

(define (load-conformance-manifest path)
  (define document (call-with-input-file path read-json))
  (unless (and (hash? document)
               (equal? (hash-ref document 'formatVersion #f) 1)
               (list? (hash-ref document 'cases #f)))
    (raise-yanshu "CONFORMANCE_INVALID_MANIFEST"
               "conformance manifest has an invalid shape"))
  document)

(define (run-conformance-manifest manifest-path)
  (define manifest (load-conformance-manifest manifest-path))
  (define manifest-root
    (or (path-only (simplify-path manifest-path #t)) (current-directory)))
  (define results
    (for/list ([case-document (in-list (hash-ref manifest 'cases))]
               [index (in-naturals 1)])
      (run-conformance-case manifest-root case-document index)))
  (define passed-count
    (count (lambda (result) (hash-ref result 'passed)) results))
  (hasheq 'formatVersion 1
          'passed (= passed-count (length results))
          'total (length results)
          'passedCount passed-count
          'failedCount (- (length results) passed-count)
          'cases results))

(define (run-conformance-case manifest-root document index)
  (unless (hash? document)
    (raise-yanshu "CONFORMANCE_INVALID_CASE"
               "conformance case must be an object"
               (hasheq 'index index)))
  (define name (required-string document 'name index))
  (define phase (required-string document 'phase index))
  (define source-relative (required-string document 'source index))
  (define source-path
    (simplify-path (build-path manifest-root (string->path source-relative)) #t))
  (unless (file-exists? source-path)
    (raise-yanshu "CONFORMANCE_SOURCE_MISSING"
               "conformance source file does not exist"
               (hasheq 'name name 'source source-relative)))
  (define expected (hash-ref document 'expect #f))
  (unless (hash? expected)
    (raise-yanshu "CONFORMANCE_INVALID_CASE"
               "conformance case requires an expected outcome"
               (hasheq 'name name)))
  (define actual
    (with-handlers
        ([exn:fail:yanshu?
          (lambda (error)
            (define public-error (hash-ref (yanshu-error->jsexpr error) 'error))
            (hasheq 'kind "diagnostic"
                    'code (hash-ref public-error 'code)
                    'message (hash-ref public-error 'message)
                    'details (hash-ref public-error 'details)))])
      (define program (load-program-file source-path))
      (cond
        [(string=? phase "load")
         (hasheq 'kind "program" 'program (program-summary program))]
        [(string=? phase "inspect")
         (hasheq 'kind "program" 'program (program-summary program))]
        [(string=? phase "run")
         (run-program-case program document name)]
        [else
         (raise-yanshu "CONFORMANCE_INVALID_PHASE"
                    "conformance case has an unknown phase"
                    (hasheq 'name name 'phase phase))])))
  (hasheq 'name name
          'passed (equal? actual expected)
          'expected expected
          'actual actual))

(define (run-program-case program document name)
  (define entry (string->symbol (required-string document 'entry name)))
  (define raw-arguments (hash-ref document 'args #f))
  (unless (list? raw-arguments)
    (raise-yanshu "CONFORMANCE_INVALID_CASE"
               "run case args must be an array"
               (hasheq 'name name)))
  (define fuel (hash-ref document 'fuel 10000))
  (define maximum-depth (hash-ref document 'maxDepth 256))
  (unless (and (exact-positive-integer? fuel)
               (exact-positive-integer? maximum-depth))
    (raise-yanshu "CONFORMANCE_INVALID_CASE"
               "run case limits must be positive integers"
               (hasheq 'name name)))
  (define arguments (map fixture->value raw-arguments))
  (define result
    (if (equal? (hash-ref document 'libraryBackends #f) "none")
        (execute-export program
                        entry
                        arguments
                        #:fuel fuel
                        #:max-depth maximum-depth
                        #:library-backends (hash))
        (execute-export program
                        entry
                        arguments
                        #:fuel fuel
                        #:max-depth maximum-depth)))
  (hasheq 'kind "value" 'value (value->fixture result)))

(define (program-summary program)
  (hasheq
   'name (symbol->string (yanshu-program-name program))
   'version (yanshu-program-version program)
   'capabilities (map symbol->string (yanshu-program-capabilities program))
   'libraries
   (for/list ([requirement (in-list (yanshu-program-libraries program))])
     (hasheq 'name (symbol->string (library-requirement-name requirement))
             'version (library-requirement-version requirement)))
   'schemas (map (lambda (schema) (symbol->string (yanshu-schema-name schema)))
                 (yanshu-program-schemas program))
   'routes
   (for/list ([route (in-list (yanshu-program-routes program))])
     (hasheq 'method (yanshu-route-method route)
             'path (yanshu-route-path route)
             'handler (symbol->string (yanshu-route-handler route))))
   'definitions
   (map (lambda (definition)
          (symbol->string (yanshu-definition-name definition)))
        (yanshu-program-definitions program))
   'exports (map symbol->string (yanshu-program-exports program))))

(define (fixture->value value)
  (cond
    [(eq? value (json-null)) '()]
    [(or (boolean? value) (string? value) (exact-integer? value)) value]
    [(list? value) (map fixture->value value)]
    [(hash? value)
     (cond
       [(and (= (hash-count value) 1) (hash-has-key? value '$int))
        (define raw (hash-ref value '$int))
        (define parsed (and (string? raw) (string->number raw 10)))
        (unless (exact-integer? parsed)
          (raise-yanshu "CONFORMANCE_INVALID_VALUE"
                     "$int fixture value must contain a decimal integer"))
        parsed]
       [(and (= (hash-count value) 1) (equal? (hash-ref value '$nil #f) #t))
        '()]
       [(and (= (hash-count value) 1) (string? (hash-ref value '$symbol #f)))
        (string->symbol (hash-ref value '$symbol))]
       [(and (= (hash-count value) 1) (hash-has-key? value '$ok))
        (ok-value (fixture->value (hash-ref value '$ok)))]
       [(and (= (hash-count value) 1) (hash-has-key? value '$err))
        (err-value (fixture->value (hash-ref value '$err)))]
       [else
        (for/hash ([(key item) (in-hash value)])
          (values (symbol->string key) (fixture->value item)))])]
    [else
     (raise-yanshu "CONFORMANCE_INVALID_VALUE"
                "fixture contains an unsupported guest value")]))

(define (value->fixture value)
  (cond
    [(null? value) (hasheq '$nil #t)]
    [(boolean? value) value]
    [(exact-integer? value) (hasheq '$int (number->string value))]
    [(string? value) value]
    [(symbol? value) (hasheq '$symbol (symbol->string value))]
    [(list? value) (map value->fixture value)]
    [(ok-value? value) (hasheq '$ok (value->fixture (ok-value-value value)))]
    [(err-value? value) (hasheq '$err (value->fixture (err-value-value value)))]
    [(hash? value)
     (for/hasheq ([(key item) (in-hash value)])
       (values (cond
                 [(symbol? key) key]
                 [(string? key) (string->symbol key)]
                 [else
                  (raise-yanshu "CONFORMANCE_UNSUPPORTED_VALUE"
                             "guest map has an unsupported fixture key")])
               (value->fixture item)))]
    [else
     (raise-yanshu "CONFORMANCE_UNSUPPORTED_VALUE"
                "guest value cannot be encoded as a conformance fixture")]))

(define (required-string document key case-name)
  (define value (hash-ref document key #f))
  (unless (and (string? value) (positive? (string-length value)))
    (raise-yanshu "CONFORMANCE_INVALID_CASE"
               "conformance case field must be a non-empty string"
               (hasheq 'case case-name 'field (symbol->string key))))
  value)
