#lang racket/base

(require racket/list
         racket/string
         "ast.rkt"
         "error.rkt"
         "library-contract.rkt"
         "schema.rkt")

(provide parse-program
         parse-expression)

(define supported-capabilities '(log kv clock))
(define supported-methods '("GET" "POST" "PUT" "PATCH" "DELETE"))
(define expression-keywords '(quote if let fn do))
(define reserved-schema-names
  '(+ - * quotient remainder = < <= > >= not
      integer? boolean? string? list? map? string-append
      list empty? length first rest map get get-or has-key? assoc
      ok err ok? err? result-value unwrap validate api-response api-error
      log now-ms kv-get kv-put kv-delete kv-list))
(define maximum-schemas 64)
(define maximum-schema-depth 16)
(define maximum-object-fields 64)
(define maximum-schema-collection-length 10000)

(define (parse-program datum source)
  (unless (and (list? datum)
               (pair? datum)
               (eq? (car datum) 'program))
    (raise-yanshu "PROGRAM_EXPECTED"
               "top-level form must be (program ...)"))
  (define name #f)
  (define version #f)
  (define capabilities #f)
  (define libraries #f)
  (define exports #f)
  (define schemas '())
  (define routes '())
  (define definitions '())
  (define schema-names (make-hasheq))
  (define definition-names (make-hasheq))
  (for ([form (in-list (cdr datum))])
    (unless (and (list? form) (pair? form) (symbol? (car form)))
      (raise-yanshu "PROGRAM_INVALID_FORM"
                 "program members must be non-empty forms"
                 (hasheq 'form (format "~s" form))))
    (case (car form)
      [(name)
       (unless (and (= (length form) 2) (symbol? (cadr form)))
         (raise-yanshu "PROGRAM_INVALID_NAME" "name must contain one symbol"))
       (when name
         (raise-yanshu "PROGRAM_DUPLICATE_NAME" "program has multiple name forms"))
       (set! name (cadr form))]
      [(version)
       (unless (and (= (length form) 2)
                    (exact-positive-integer? (cadr form)))
         (raise-yanshu "PROGRAM_INVALID_VERSION"
                    "version must contain one positive integer"))
       (when version
         (raise-yanshu "PROGRAM_DUPLICATE_VERSION"
                    "program has multiple version forms"))
       (set! version (cadr form))]
      [(capabilities)
       (when capabilities
         (raise-yanshu "PROGRAM_DUPLICATE_CAPABILITIES"
                    "program has multiple capabilities forms"))
       (define values (cdr form))
       (unless (andmap symbol? values)
         (raise-yanshu "PROGRAM_INVALID_CAPABILITY"
                    "capability names must be symbols"))
       (ensure-unique-symbols values
                              "PROGRAM_DUPLICATE_CAPABILITY"
                              "capability is declared more than once")
       (for ([capability (in-list values)])
         (unless (memq capability supported-capabilities)
           (raise-yanshu "PROGRAM_UNKNOWN_CAPABILITY"
                      "program declares an unsupported capability"
                      (hasheq 'capability (symbol->string capability)))))
       (set! capabilities values)]
      [(libraries)
       (when libraries
         (raise-yanshu "PROGRAM_DUPLICATE_LIBRARIES"
                    "program has multiple libraries forms"))
       (define declarations (cdr form))
       (when (> (length declarations) maximum-library-count)
         (raise-yanshu "PROGRAM_TOO_MANY_LIBRARIES"
                    "program declares too many libraries"
                    (hasheq 'maximum maximum-library-count)))
       (define seen-libraries (make-hasheq))
       (set!
        libraries
        (for/list ([declaration (in-list declarations)])
          (unless (and (list? declaration)
                       (= (length declaration) 2)
                       (valid-library-name? (car declaration))
                       (exact-positive-integer? (cadr declaration))
                       (<= (cadr declaration) maximum-library-version))
            (raise-yanshu
             "PROGRAM_INVALID_LIBRARY"
             "library declaration must be (lowercase-name VERSION)"
             (hasheq 'library (format "~s" declaration))))
          (define library-name (car declaration))
          (define library-version (cadr declaration))
          (when (hash-has-key? seen-libraries library-name)
            (raise-yanshu "PROGRAM_DUPLICATE_LIBRARY"
                       "program declares a library more than once"
                       (hasheq 'library (symbol->string library-name))))
          (hash-set! seen-libraries library-name #t)
          (unless (find-library-contract library-name library-version)
            (raise-yanshu "PROGRAM_UNKNOWN_LIBRARY"
                       "program declares an unsupported library contract"
                       (hasheq 'library (symbol->string library-name)
                               'version library-version)))
          (library-requirement library-name library-version)))]
      [(schema)
       (unless (and (= (length form) 3) (symbol? (cadr form)))
         (raise-yanshu "PROGRAM_INVALID_SCHEMA"
                    "schema must be (schema name specification)"))
       (when (>= (length schemas) maximum-schemas)
         (raise-yanshu "PROGRAM_TOO_MANY_SCHEMAS"
                    "program declares too many schemas"
                    (hasheq 'maximum maximum-schemas)))
       (define schema-name (cadr form))
       (when (memq schema-name reserved-schema-names)
         (raise-yanshu "PROGRAM_SCHEMA_RESERVED_NAME"
                    "schema name conflicts with a language or capability binding"
                    (hasheq 'name (symbol->string schema-name))))
       (when (hash-has-key? schema-names schema-name)
         (raise-yanshu "PROGRAM_DUPLICATE_SCHEMA"
                    "schema name is not unique"
                    (hasheq 'name (symbol->string schema-name))))
       (when (hash-has-key? definition-names schema-name)
         (raise-yanshu "PROGRAM_DUPLICATE_BINDING"
                    "schema and definition names must be unique"
                    (hasheq 'name (symbol->string schema-name))))
       (hash-set! schema-names schema-name #t)
       (set! schemas
             (cons (yanshu-schema schema-name
                               (parse-schema-specification (caddr form) 0))
                   schemas))]
      [(route)
       (unless (and (= (length form) 4)
                    (symbol? (cadr form))
                    (string? (caddr form))
                    (symbol? (cadddr form)))
         (raise-yanshu "PROGRAM_INVALID_ROUTE"
                    "route must be (route METHOD \"/path\" handler)"))
       (define method (string-upcase (symbol->string (cadr form))))
       (define path (caddr form))
       (define handler (cadddr form))
       (unless (member method supported-methods)
         (raise-yanshu "PROGRAM_UNSUPPORTED_METHOD"
                    "route uses an unsupported HTTP method"
                    (hasheq 'method method)))
       (validate-route-path path)
       (for ([existing (in-list routes)])
         (when (and (string=? method (yanshu-route-method existing))
                    (route-patterns-overlap? path (yanshu-route-path existing)))
           (raise-yanshu "PROGRAM_AMBIGUOUS_ROUTE"
                      "route overlaps an earlier route for the same method"
                      (hasheq 'method method
                              'path path
                              'existingPath (yanshu-route-path existing)))))
       (set! routes (cons (yanshu-route method path handler) routes))]
      [(def)
       (unless (and (= (length form) 3) (symbol? (cadr form)))
         (raise-yanshu "PROGRAM_INVALID_DEFINITION"
                    "definition must be (def name expression)"))
       (define definition-name (cadr form))
       (when (hash-has-key? definition-names definition-name)
         (raise-yanshu "PROGRAM_DUPLICATE_DEFINITION"
                    "definition name is not unique"
                    (hasheq 'name (symbol->string definition-name))))
       (when (hash-has-key? schema-names definition-name)
         (raise-yanshu "PROGRAM_DUPLICATE_BINDING"
                    "schema and definition names must be unique"
                    (hasheq 'name (symbol->string definition-name))))
       (hash-set! definition-names definition-name #t)
       (set! definitions
             (cons (yanshu-definition definition-name
                                   (parse-expression (caddr form)))
                   definitions))]
      [(export)
       (when exports
         (raise-yanshu "PROGRAM_DUPLICATE_EXPORT"
                    "program has multiple export forms"))
       (define values (cdr form))
       (unless (and (pair? values) (andmap symbol? values))
         (raise-yanshu "PROGRAM_INVALID_EXPORT"
                    "export must contain at least one symbol"))
       (ensure-unique-symbols values
                              "PROGRAM_DUPLICATE_EXPORT_NAME"
                              "export name is listed more than once")
       (set! exports values)]
      [else
       (raise-yanshu "PROGRAM_UNKNOWN_FORM"
                  "unknown top-level program form"
                  (hasheq 'form (symbol->string (car form))))]))
  (unless name
    (raise-yanshu "PROGRAM_MISSING_NAME" "program is missing a name form"))
  (unless version
    (raise-yanshu "PROGRAM_MISSING_VERSION" "program is missing a version form"))
  (unless exports
    (raise-yanshu "PROGRAM_MISSING_EXPORT" "program is missing an export form"))
  (for ([export-name (in-list exports)])
    (unless (hash-has-key? definition-names export-name)
      (raise-yanshu "PROGRAM_UNKNOWN_EXPORT"
                 "export does not name a program definition"
                 (hasheq 'name (symbol->string export-name)))))
  (for ([route (in-list routes)])
    (unless (hash-has-key? definition-names (yanshu-route-handler route))
      (raise-yanshu "PROGRAM_UNKNOWN_ROUTE_HANDLER"
                 "route handler does not name a program definition"
                 (hasheq 'handler
                         (symbol->string (yanshu-route-handler route)))))
    (unless (memq (yanshu-route-handler route) exports)
      (raise-yanshu "PROGRAM_ROUTE_HANDLER_NOT_EXPORTED"
                 "route handler must be exported"
                 (hasheq 'handler
                         (symbol->string (yanshu-route-handler route))))))
  (for ([requirement (in-list (or libraries '()))])
    (define namespace-prefix
      (string-append (symbol->string (library-requirement-name requirement))
                     "/"))
    (for ([binding-name
           (in-list
            (append (map yanshu-schema-name schemas)
                    (map yanshu-definition-name definitions)))])
      (when (string-prefix? (symbol->string binding-name) namespace-prefix)
        (raise-yanshu "PROGRAM_LIBRARY_NAMESPACE_CONFLICT"
                   "guest binding occupies a declared library namespace"
                   (hasheq
                    'library
                    (symbol->string (library-requirement-name requirement))
                    'binding (symbol->string binding-name))))))
  (yanshu-program name
               version
               (or capabilities '())
               (or libraries '())
               (reverse schemas)
               (reverse routes)
               (reverse definitions)
               exports
               source))

(define (parse-schema-specification datum depth)
  (when (> depth maximum-schema-depth)
    (raise-yanshu "PROGRAM_SCHEMA_TOO_DEEP"
               "schema exceeds the maximum nesting depth"
               (hasheq 'maximum maximum-schema-depth)))
  (cond
    [(eq? datum 'any) (schema-any)]
    [(eq? datum 'string) (schema-string 0 #f)]
    [(eq? datum 'integer) (schema-integer #f #f)]
    [(eq? datum 'boolean) (schema-boolean)]
    [(and (list? datum) (pair? datum))
     (case (car datum)
       [(string)
        (unless (and (= (length datum) 3)
                     (exact-nonnegative-integer? (cadr datum))
                     (exact-nonnegative-integer? (caddr datum))
                     (<= (cadr datum) (caddr datum)))
          (invalid-schema-specification
           datum
           "bounded string must be (string MINIMUM MAXIMUM)"))
        (schema-string (cadr datum) (caddr datum))]
       [(integer)
        (unless (and (= (length datum) 3)
                     (exact-integer? (cadr datum))
                     (exact-integer? (caddr datum))
                     (<= (cadr datum) (caddr datum)))
          (invalid-schema-specification
           datum
           "bounded integer must be (integer MINIMUM MAXIMUM)"))
        (schema-integer (cadr datum) (caddr datum))]
       [(list)
        (unless (and (= (length datum) 4)
                     (exact-nonnegative-integer? (caddr datum))
                     (exact-nonnegative-integer? (cadddr datum))
                     (<= (caddr datum) (cadddr datum))
                     (<= (cadddr datum) maximum-schema-collection-length))
          (invalid-schema-specification
           datum
           "list must be (list ITEM MINIMUM MAXIMUM) with bounded lengths"))
        (schema-list
         (parse-schema-specification (cadr datum) (add1 depth))
         (caddr datum)
         (cadddr datum))]
       [(object)
        (define raw-fields (cdr datum))
        (when (> (length raw-fields) maximum-object-fields)
          (raise-yanshu "PROGRAM_SCHEMA_TOO_MANY_FIELDS"
                     "object schema declares too many fields"
                     (hasheq 'maximum maximum-object-fields)))
        (define fields
          (for/list ([raw-field (in-list raw-fields)])
            (parse-schema-field raw-field (add1 depth))))
        (define field-names (map schema-field-name fields))
        (define duplicate (check-duplicates field-names string=?))
        (when duplicate
          (raise-yanshu "PROGRAM_SCHEMA_DUPLICATE_FIELD"
                     "object schema field name is not unique"
                     (hasheq 'field duplicate)))
        (schema-object fields)]
       [else
        (invalid-schema-specification datum "unknown schema constructor")])]
    [else
     (invalid-schema-specification datum "unknown schema specification")]))

(define (parse-schema-field datum depth)
  (unless (and (list? datum)
               (pair? datum)
               (>= (length datum) 3)
               (memq (car datum) '(required optional))
               (string? (cadr datum))
               (positive? (string-length (cadr datum)))
               (<= (string-length (cadr datum)) 128))
    (raise-yanshu "PROGRAM_INVALID_SCHEMA_FIELD"
               "schema field must have a bounded string name"
               (hasheq 'field (format "~s" datum))))
  (define required? (eq? (car datum) 'required))
  (unless (if required?
              (= (length datum) 3)
              (member (length datum) '(3 4)))
    (raise-yanshu
     "PROGRAM_INVALID_SCHEMA_FIELD"
     (if required?
         "required field must be (required \"name\" SCHEMA)"
         "optional field must be (optional \"name\" SCHEMA [DEFAULT])")
     (hasheq 'field (format "~s" datum))))
  (define specification
    (parse-schema-specification (caddr datum) depth))
  (define has-default? (= (length datum) 4))
  (define default (and has-default? (cadddr datum)))
  (when has-default?
    (unless (schema-default-datum? default)
      (raise-yanshu "PROGRAM_SCHEMA_INVALID_DEFAULT"
                 "schema default must be a portable literal"
                 (hasheq 'field (cadr datum))))
    (define validation (validate-schema specification default))
    (unless (schema-validation-valid? validation)
      (raise-yanshu "PROGRAM_SCHEMA_INVALID_DEFAULT"
                 "schema default does not satisfy its field schema"
                 (hasheq 'field (cadr datum)
                         'issue (car (schema-validation-issues validation))))))
  (schema-field (cadr datum)
                specification
                required?
                has-default?
                default))

(define (schema-default-datum? value)
  (cond
    [(or (exact-integer? value) (boolean? value) (string? value) (null? value)) #t]
    [(list? value) (andmap schema-default-datum? value)]
    [else #f]))

(define (invalid-schema-specification datum message)
  (raise-yanshu "PROGRAM_INVALID_SCHEMA_SPECIFICATION"
             message
             (hasheq 'schema (format "~s" datum))))

(define (parse-expression datum)
  (cond
    [(or (exact-integer? datum) (boolean? datum) (string? datum) (null? datum))
     (expr-lit datum)]
    [(symbol? datum) (expr-var datum)]
    [(and (list? datum) (pair? datum))
     (define head (car datum))
     (cond
       [(eq? head 'quote)
        (unless (= (length datum) 2)
          (invalid-special-form 'quote datum))
        (expr-quote (cadr datum))]
       [(eq? head 'if)
        (unless (= (length datum) 4)
          (invalid-special-form 'if datum))
        (expr-if (parse-expression (cadr datum))
                 (parse-expression (caddr datum))
                 (parse-expression (cadddr datum)))]
       [(eq? head 'let)
        (unless (= (length datum) 3)
          (invalid-special-form 'let datum))
        (define raw-bindings (cadr datum))
        (unless (list? raw-bindings)
          (raise-yanshu "PARSE_INVALID_LET_BINDINGS"
                     "let bindings must be a proper list"))
        (define binding-names
          (for/list ([binding (in-list raw-bindings)])
            (unless (and (list? binding)
                         (= (length binding) 2)
                         (symbol? (car binding)))
              (raise-yanshu "PARSE_INVALID_LET_BINDING"
                         "let binding must be (name expression)"
                         (hasheq 'binding (format "~s" binding))))
            (car binding)))
        (ensure-unique-symbols binding-names
                               "PARSE_DUPLICATE_LET_BINDING"
                               "let binding name is not unique")
        (expr-let
         (for/list ([binding (in-list raw-bindings)])
           (ast-binding (car binding)
                        (parse-expression (cadr binding))))
         (parse-expression (caddr datum)))]
       [(eq? head 'fn)
        (unless (= (length datum) 3)
          (invalid-special-form 'fn datum))
        (define parameters (cadr datum))
        (unless (and (list? parameters) (andmap symbol? parameters))
          (raise-yanshu "PARSE_INVALID_PARAMETERS"
                     "function parameters must be a proper list of symbols"))
        (ensure-unique-symbols parameters
                               "PARSE_DUPLICATE_PARAMETER"
                               "function parameter name is not unique")
        (expr-fn parameters (parse-expression (caddr datum)))]
       [(eq? head 'do)
        (when (= (length datum) 1)
          (invalid-special-form 'do datum))
        (expr-do (map parse-expression (cdr datum)))]
       [(and (symbol? head) (memq head expression-keywords))
        (invalid-special-form head datum)]
       [else
        (expr-call (parse-expression head)
                   (map parse-expression (cdr datum)))])]
    [else
     (raise-yanshu "PARSE_INVALID_EXPRESSION"
                "datum cannot be used as an expression"
                (hasheq 'datum (format "~s" datum)))]))

(define (ensure-unique-symbols symbols code message)
  (define seen (make-hasheq))
  (for ([symbol (in-list symbols)])
    (when (hash-has-key? seen symbol)
      (raise-yanshu code message (hasheq 'name (symbol->string symbol))))
    (hash-set! seen symbol #t)))

(define (invalid-special-form name datum)
  (raise-yanshu "PARSE_INVALID_SPECIAL_FORM"
             "special form has an invalid shape"
             (hasheq 'form (symbol->string name)
                     'datum (format "~s" datum))))

(define (validate-route-path path)
  (unless (and (positive? (string-length path))
               (<= (string-length path) 2048)
               (char=? (string-ref path 0) #\/)
               (not (string-contains? path "?"))
               (not (string-contains? path "#"))
               (not (for/or ([character (in-string path)])
                      (char-whitespace? character))))
    (raise-yanshu "PROGRAM_INVALID_ROUTE_PATH"
               "route path must be an absolute path without query or fragment"
               (hasheq 'path path)))
  (define segments (route-segments path))
  (when (member "" segments)
    (raise-yanshu "PROGRAM_INVALID_ROUTE_PATH"
               "route path cannot contain empty segments or a trailing slash"
               (hasheq 'path path)))
  (define parameter-names '())
  (for ([segment (in-list segments)])
    (when (string-prefix? segment ":")
      (unless (regexp-match? #px"^:[A-Za-z_][A-Za-z0-9_-]*$" segment)
        (raise-yanshu "PROGRAM_INVALID_ROUTE_PARAMETER"
                   "route parameter has an invalid name"
                   (hasheq 'path path 'segment segment)))
      (define parameter-name (substring segment 1))
      (when (member parameter-name parameter-names)
        (raise-yanshu "PROGRAM_DUPLICATE_ROUTE_PARAMETER"
                   "route parameter name is repeated"
                   (hasheq 'path path 'parameter parameter-name)))
      (set! parameter-names (cons parameter-name parameter-names)))))

(define (route-segments path)
  (if (string=? path "/")
      '()
      (string-split (substring path 1) "/" #:trim? #f)))

(define (route-parameter-segment? segment)
  (and (positive? (string-length segment))
       (char=? (string-ref segment 0) #\:)))

(define (route-patterns-overlap? left right)
  (define left-segments (route-segments left))
  (define right-segments (route-segments right))
  (and (= (length left-segments) (length right-segments))
       (for/and ([left-segment (in-list left-segments)]
                 [right-segment (in-list right-segments)])
         (or (route-parameter-segment? left-segment)
             (route-parameter-segment? right-segment)
             (string=? left-segment right-segment)))))
