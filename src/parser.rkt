#lang racket/base

(require racket/list
         racket/string
         "ast.rkt"
         "error.rkt")

(provide parse-program
         parse-expression)

(define supported-capabilities '(log kv clock))
(define supported-methods '("GET" "POST" "PUT" "PATCH" "DELETE"))
(define expression-keywords '(quote if let fn do))

(define (parse-program datum source)
  (unless (and (list? datum)
               (pair? datum)
               (eq? (car datum) 'program))
    (raise-ail "PROGRAM_EXPECTED"
               "top-level form must be (program ...)"))
  (define name #f)
  (define version #f)
  (define capabilities #f)
  (define exports #f)
  (define routes '())
  (define definitions '())
  (define definition-names (make-hasheq))
  (for ([form (in-list (cdr datum))])
    (unless (and (list? form) (pair? form) (symbol? (car form)))
      (raise-ail "PROGRAM_INVALID_FORM"
                 "program members must be non-empty forms"
                 (hasheq 'form (format "~s" form))))
    (case (car form)
      [(name)
       (unless (and (= (length form) 2) (symbol? (cadr form)))
         (raise-ail "PROGRAM_INVALID_NAME" "name must contain one symbol"))
       (when name
         (raise-ail "PROGRAM_DUPLICATE_NAME" "program has multiple name forms"))
       (set! name (cadr form))]
      [(version)
       (unless (and (= (length form) 2)
                    (exact-positive-integer? (cadr form)))
         (raise-ail "PROGRAM_INVALID_VERSION"
                    "version must contain one positive integer"))
       (when version
         (raise-ail "PROGRAM_DUPLICATE_VERSION"
                    "program has multiple version forms"))
       (set! version (cadr form))]
      [(capabilities)
       (when capabilities
         (raise-ail "PROGRAM_DUPLICATE_CAPABILITIES"
                    "program has multiple capabilities forms"))
       (define values (cdr form))
       (unless (andmap symbol? values)
         (raise-ail "PROGRAM_INVALID_CAPABILITY"
                    "capability names must be symbols"))
       (ensure-unique-symbols values
                              "PROGRAM_DUPLICATE_CAPABILITY"
                              "capability is declared more than once")
       (for ([capability (in-list values)])
         (unless (memq capability supported-capabilities)
           (raise-ail "PROGRAM_UNKNOWN_CAPABILITY"
                      "program declares an unsupported capability"
                      (hasheq 'capability (symbol->string capability)))))
       (set! capabilities values)]
      [(route)
       (unless (and (= (length form) 4)
                    (symbol? (cadr form))
                    (string? (caddr form))
                    (symbol? (cadddr form)))
         (raise-ail "PROGRAM_INVALID_ROUTE"
                    "route must be (route METHOD \"/path\" handler)"))
       (define method (string-upcase (symbol->string (cadr form))))
       (define path (caddr form))
       (define handler (cadddr form))
       (unless (member method supported-methods)
         (raise-ail "PROGRAM_UNSUPPORTED_METHOD"
                    "route uses an unsupported HTTP method"
                    (hasheq 'method method)))
       (validate-route-path path)
       (for ([existing (in-list routes)])
         (when (and (string=? method (ail-route-method existing))
                    (route-patterns-overlap? path (ail-route-path existing)))
           (raise-ail "PROGRAM_AMBIGUOUS_ROUTE"
                      "route overlaps an earlier route for the same method"
                      (hasheq 'method method
                              'path path
                              'existingPath (ail-route-path existing)))))
       (set! routes (cons (ail-route method path handler) routes))]
      [(def)
       (unless (and (= (length form) 3) (symbol? (cadr form)))
         (raise-ail "PROGRAM_INVALID_DEFINITION"
                    "definition must be (def name expression)"))
       (define definition-name (cadr form))
       (when (hash-has-key? definition-names definition-name)
         (raise-ail "PROGRAM_DUPLICATE_DEFINITION"
                    "definition name is not unique"
                    (hasheq 'name (symbol->string definition-name))))
       (hash-set! definition-names definition-name #t)
       (set! definitions
             (cons (ail-definition definition-name
                                   (parse-expression (caddr form)))
                   definitions))]
      [(export)
       (when exports
         (raise-ail "PROGRAM_DUPLICATE_EXPORT"
                    "program has multiple export forms"))
       (define values (cdr form))
       (unless (and (pair? values) (andmap symbol? values))
         (raise-ail "PROGRAM_INVALID_EXPORT"
                    "export must contain at least one symbol"))
       (ensure-unique-symbols values
                              "PROGRAM_DUPLICATE_EXPORT_NAME"
                              "export name is listed more than once")
       (set! exports values)]
      [else
       (raise-ail "PROGRAM_UNKNOWN_FORM"
                  "unknown top-level program form"
                  (hasheq 'form (symbol->string (car form))))]))
  (unless name
    (raise-ail "PROGRAM_MISSING_NAME" "program is missing a name form"))
  (unless version
    (raise-ail "PROGRAM_MISSING_VERSION" "program is missing a version form"))
  (unless exports
    (raise-ail "PROGRAM_MISSING_EXPORT" "program is missing an export form"))
  (for ([export-name (in-list exports)])
    (unless (hash-has-key? definition-names export-name)
      (raise-ail "PROGRAM_UNKNOWN_EXPORT"
                 "export does not name a program definition"
                 (hasheq 'name (symbol->string export-name)))))
  (for ([route (in-list routes)])
    (unless (hash-has-key? definition-names (ail-route-handler route))
      (raise-ail "PROGRAM_UNKNOWN_ROUTE_HANDLER"
                 "route handler does not name a program definition"
                 (hasheq 'handler
                         (symbol->string (ail-route-handler route)))))
    (unless (memq (ail-route-handler route) exports)
      (raise-ail "PROGRAM_ROUTE_HANDLER_NOT_EXPORTED"
                 "route handler must be exported"
                 (hasheq 'handler
                         (symbol->string (ail-route-handler route))))))
  (ail-program name
               version
               (or capabilities '())
               (reverse routes)
               (reverse definitions)
               exports
               source))

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
          (raise-ail "PARSE_INVALID_LET_BINDINGS"
                     "let bindings must be a proper list"))
        (define binding-names
          (for/list ([binding (in-list raw-bindings)])
            (unless (and (list? binding)
                         (= (length binding) 2)
                         (symbol? (car binding)))
              (raise-ail "PARSE_INVALID_LET_BINDING"
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
          (raise-ail "PARSE_INVALID_PARAMETERS"
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
     (raise-ail "PARSE_INVALID_EXPRESSION"
                "datum cannot be used as an expression"
                (hasheq 'datum (format "~s" datum)))]))

(define (ensure-unique-symbols symbols code message)
  (define seen (make-hasheq))
  (for ([symbol (in-list symbols)])
    (when (hash-has-key? seen symbol)
      (raise-ail code message (hasheq 'name (symbol->string symbol))))
    (hash-set! seen symbol #t)))

(define (invalid-special-form name datum)
  (raise-ail "PARSE_INVALID_SPECIAL_FORM"
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
    (raise-ail "PROGRAM_INVALID_ROUTE_PATH"
               "route path must be an absolute path without query or fragment"
               (hasheq 'path path)))
  (define segments (route-segments path))
  (when (member "" segments)
    (raise-ail "PROGRAM_INVALID_ROUTE_PATH"
               "route path cannot contain empty segments or a trailing slash"
               (hasheq 'path path)))
  (define parameter-names '())
  (for ([segment (in-list segments)])
    (when (string-prefix? segment ":")
      (unless (regexp-match? #px"^:[A-Za-z_][A-Za-z0-9_-]*$" segment)
        (raise-ail "PROGRAM_INVALID_ROUTE_PARAMETER"
                   "route parameter has an invalid name"
                   (hasheq 'path path 'segment segment)))
      (define parameter-name (substring segment 1))
      (when (member parameter-name parameter-names)
        (raise-ail "PROGRAM_DUPLICATE_ROUTE_PARAMETER"
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
