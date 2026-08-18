#lang racket/base

(require json
         racket/file
         racket/list
         "ast.rkt"
         "error.rkt"
         "library-backend.rkt"
         "library-contract.rkt"
         "parser.rkt"
         "reader.rkt"
         "schema.rkt")

(provide (struct-out ok-value)
         (struct-out err-value)
         (struct-out capability-primitive)
         (all-from-out "library-backend.rkt")
         load-program-source
         load-program-file
         execute-export
         value->jsexpr
         jsexpr->value)

(struct environment (bindings parent) #:transparent)
(struct closure (parameters body environment) #:transparent)
(struct primitive (name minimum-arity maximum-arity implementation) #:transparent)
(struct execution-context (fuel maximum-depth logger) #:mutable #:transparent)
(struct ok-value (value) #:transparent)
(struct err-value (value) #:transparent)
(struct schema-value (name specification) #:transparent)
(struct capability-primitive (minimum-arity maximum-arity implementation)
  #:transparent)

(define (load-program-source source)
  (parse-program (read-source source) source))

(define (load-program-file path)
  (load-program-source (file->string path)))

(define (execute-export program export-name arguments
                        #:fuel [fuel 10000]
                        #:max-depth [maximum-depth 256]
                        #:logger [logger default-logger]
                        #:library-backends
                        [library-backends (make-reference-library-backends)]
                        #:capability-bindings [capability-bindings (hasheq)])
  (unless (and (exact-integer? fuel) (positive? fuel))
    (raise-argument-error 'execute-export "exact-positive-integer?" fuel))
  (unless (and (exact-integer? maximum-depth) (positive? maximum-depth))
    (raise-argument-error 'execute-export
                          "exact-positive-integer?"
                          maximum-depth))
  (unless (memq export-name (ail-program-exports program))
    (raise-ail "RUNTIME_NOT_EXPORTED"
               "requested entry point is not exported"
               (hasheq 'name (symbol->string export-name))))
  (define context (execution-context fuel maximum-depth logger))
  (define base-environment (make-base-environment context))
  (define module-environment
    (environment (make-hasheq) base-environment))
  (install-libraries! module-environment
                      (ail-program-libraries program)
                      context
                      library-backends)
  (install-capabilities! module-environment
                         (ail-program-capabilities program)
                         context
                         capability-bindings)
  (for ([schema (in-list (ail-program-schemas program))])
    (environment-define!
     module-environment
     (ail-schema-name schema)
     (schema-value (ail-schema-name schema)
                   (ail-schema-specification schema))))
  (for ([definition (in-list (ail-program-definitions program))])
    (environment-define!
     module-environment
     (ail-definition-name definition)
     (evaluate (ail-definition-expression definition)
               module-environment
               context
               0)))
  (apply-callable (environment-lookup module-environment export-name)
                  arguments
                  context
                  0))

(define (evaluate expression current-environment context depth)
  (consume-fuel! context)
  (check-depth! context depth)
  (cond
    [(expr-lit? expression) (expr-lit-value expression)]
    [(expr-var? expression)
     (environment-lookup current-environment (expr-var-name expression))]
    [(expr-quote? expression) (expr-quote-datum expression)]
    [(expr-if? expression)
     (if (eq? #f (evaluate (expr-if-condition expression)
                           current-environment
                           context
                           depth))
         (evaluate (expr-if-alternative expression)
                   current-environment
                   context
                   depth)
         (evaluate (expr-if-consequent expression)
                   current-environment
                   context
                   depth))]
    [(expr-let? expression)
     (define local-environment
       (environment (make-hasheq) current-environment))
     (for ([binding (in-list (expr-let-bindings expression))])
       (environment-define!
        local-environment
        (ast-binding-name binding)
        (evaluate (ast-binding-expression binding)
                  local-environment
                  context
                  depth)))
     (evaluate (expr-let-body expression)
               local-environment
               context
               depth)]
    [(expr-fn? expression)
     (closure (expr-fn-parameters expression)
              (expr-fn-body expression)
              current-environment)]
    [(expr-do? expression)
     (for/fold ([result '()])
               ([item (in-list (expr-do-expressions expression))])
       (evaluate item current-environment context depth))]
    [(expr-call? expression)
     (define callable
       (evaluate (expr-call-callee expression)
                 current-environment
                 context
                 depth))
     (define arguments
       (for/list ([argument (in-list (expr-call-arguments expression))])
         (evaluate argument current-environment context depth)))
     (apply-callable callable arguments context (add1 depth))]
    [else
     (raise-ail "RUNTIME_UNKNOWN_AST"
                "interpreter received an unknown AST node")]))

(define (apply-callable callable arguments context depth)
  (consume-fuel! context)
  (check-depth! context depth)
  (cond
    [(closure? callable)
     (define parameters (closure-parameters callable))
     (unless (= (length parameters) (length arguments))
       (raise-arity-error "function"
                          (length parameters)
                          (length parameters)
                          (length arguments)))
     (define call-environment
       (environment (make-hasheq) (closure-environment callable)))
     (for ([parameter (in-list parameters)]
           [argument (in-list arguments)])
       (environment-define! call-environment parameter argument))
     (evaluate (closure-body callable)
               call-environment
               context
               depth)]
    [(primitive? callable)
     (check-primitive-arity! callable arguments)
     ((primitive-implementation callable) arguments context)]
    [else
     (raise-ail "RUNTIME_NOT_CALLABLE"
                "attempted to call a non-callable value"
                (hasheq 'kind (value-kind callable)))]))

(define (make-base-environment context)
  (define base (environment (make-hasheq) #f))
  (define (install name minimum maximum implementation)
    (environment-define!
     base
     name
     (primitive name minimum maximum implementation)))

  (install '+ 0 #f
           (lambda (arguments _context)
             (for/sum ([argument (in-list arguments)])
               (expect-integer '+ argument))))
  (install '* 0 #f
           (lambda (arguments _context)
             (for/fold ([result 1]) ([argument (in-list arguments)])
               (* result (expect-integer '* argument)))))
  (install '- 1 #f
           (lambda (arguments _context)
             (define integers
               (map (lambda (value) (expect-integer '- value)) arguments))
             (if (= (length integers) 1)
                 (- (car integers))
                 (foldl (lambda (value result) (- result value))
                        (car integers)
                        (cdr integers)))))
  (install 'quotient 2 2
           (lambda (arguments _context)
             (define numerator (expect-integer 'quotient (car arguments)))
             (define denominator (expect-integer 'quotient (cadr arguments)))
             (when (zero? denominator)
               (raise-ail "RUNTIME_DIVIDE_BY_ZERO"
                          "quotient denominator cannot be zero"))
             (quotient numerator denominator)))
  (install 'remainder 2 2
           (lambda (arguments _context)
             (define numerator (expect-integer 'remainder (car arguments)))
             (define denominator (expect-integer 'remainder (cadr arguments)))
             (when (zero? denominator)
               (raise-ail "RUNTIME_DIVIDE_BY_ZERO"
                          "remainder denominator cannot be zero"))
             (remainder numerator denominator)))
  (install '= 2 2
           (lambda (arguments _context)
             (equal? (car arguments) (cadr arguments))))
  (for ([entry
         (in-list
          (list (cons '< <)
                (cons '<= <=)
                (cons '> >)
                (cons '>= >=)))])
    (define name (car entry))
    (define operation (cdr entry))
    (install name 2 2
             (lambda (arguments _context)
               (operation (expect-integer name (car arguments))
                          (expect-integer name (cadr arguments))))))
  (install 'not 1 1
           (lambda (arguments _context) (eq? #f (car arguments))))
  (install 'integer? 1 1
           (lambda (arguments _context) (exact-integer? (car arguments))))
  (install 'boolean? 1 1
           (lambda (arguments _context) (boolean? (car arguments))))
  (install 'string? 1 1
           (lambda (arguments _context) (string? (car arguments))))
  (install 'list? 1 1
           (lambda (arguments _context) (list? (car arguments))))
  (install 'map? 1 1
           (lambda (arguments _context) (hash? (car arguments))))
  (install 'string-append 0 #f
           (lambda (arguments _context)
             (apply string-append
                    (map (lambda (value)
                           (unless (string? value)
                             (raise-type-error 'string-append "String" value))
                           value)
                         arguments))))

  (install 'list 0 #f (lambda (arguments _context) arguments))
  (install 'empty? 1 1
           (lambda (arguments _context)
             (null? (expect-list 'empty? (car arguments)))))
  (install 'length 1 1
           (lambda (arguments _context)
             (length (expect-list 'length (car arguments)))))
  (install 'first 1 1
           (lambda (arguments _context)
             (define values (expect-list 'first (car arguments)))
             (when (null? values)
               (raise-ail "RUNTIME_EMPTY_COLLECTION"
                          "first cannot read an empty list"))
             (car values)))
  (install 'rest 1 1
           (lambda (arguments _context)
             (define values (expect-list 'rest (car arguments)))
             (when (null? values)
               (raise-ail "RUNTIME_EMPTY_COLLECTION"
                          "rest cannot read an empty list"))
             (cdr values)))
  (install 'map 0 #f
           (lambda (arguments _context)
             (unless (even? (length arguments))
               (raise-ail "RUNTIME_MAP_ARITY"
                          "map expects alternating key and value arguments"))
             (for/fold ([result (hash)])
                       ([index (in-range 0 (length arguments) 2)])
               (define key (list-ref arguments index))
               (expect-map-key 'map key)
               (hash-set result key (list-ref arguments (add1 index))))))
  (install 'get 2 2
           (lambda (arguments _context)
             (define mapping (expect-map 'get (car arguments)))
             (define key (cadr arguments))
             (unless (hash-has-key? mapping key)
               (raise-ail "RUNTIME_MISSING_KEY"
                          "map does not contain the requested key"
                          (hasheq 'key (format "~s" key))))
             (hash-ref mapping key)))
  (install 'get-or 3 3
           (lambda (arguments _context)
             (define mapping (expect-map 'get-or (car arguments)))
             (hash-ref mapping (cadr arguments) (lambda () (caddr arguments)))))
  (install 'has-key? 2 2
           (lambda (arguments _context)
             (define mapping (expect-map 'has-key? (car arguments)))
             (hash-has-key? mapping (cadr arguments))))
  (install 'assoc 3 3
           (lambda (arguments _context)
             (define mapping (expect-map 'assoc (car arguments)))
             (define key (cadr arguments))
             (expect-map-key 'assoc key)
             (hash-set mapping key (caddr arguments))))

  (install 'validate 2 2
           (lambda (arguments runtime-context)
             (define selected (car arguments))
             (unless (schema-value? selected)
               (raise-type-error 'validate "Schema" selected))
             (define validation
               (validate-schema
                (schema-value-specification selected)
                (cadr arguments)
                #:step (lambda () (consume-fuel! runtime-context))))
             (if (schema-validation-valid? validation)
                 (ok-value (schema-validation-value validation))
                 (err-value (schema-validation-issues validation)))))

  (install 'ok 1 1
           (lambda (arguments _context) (ok-value (car arguments))))
  (install 'err 1 1
           (lambda (arguments _context) (err-value (car arguments))))
  (install 'ok? 1 1
           (lambda (arguments _context) (ok-value? (car arguments))))
  (install 'err? 1 1
           (lambda (arguments _context) (err-value? (car arguments))))
  (install 'result-value 1 1
           (lambda (arguments _context)
             (define value (car arguments))
             (cond
               [(ok-value? value) (ok-value-value value)]
               [(err-value? value) (err-value-value value)]
               [else (raise-type-error 'result-value "Result" value)])))
  (install 'unwrap 1 1
           (lambda (arguments _context)
             (define value (car arguments))
             (cond
               [(ok-value? value) (ok-value-value value)]
               [(err-value? value)
                (raise-ail "RUNTIME_UNWRAP_ERROR"
                           "cannot unwrap an Err value"
                           (hasheq 'value
                                   (value->jsexpr (err-value-value value))))]
               [else
                (raise-type-error 'unwrap "Result" value)])))
  (install 'api-response 2 2
           (lambda (arguments _context)
             (define status (expect-http-status 'api-response (car arguments) 100))
             (hash "status" status
                   "headers" (hash)
                   "body" (cadr arguments))))
  (install 'api-error 3 4
           (lambda (arguments _context)
             (define status (expect-http-status 'api-error (car arguments) 400))
             (define code (cadr arguments))
             (define message (caddr arguments))
             (unless (and (string? code)
                          (<= 1 (string-length code) 128)
                          (regexp-match? #px"^[A-Z][A-Z0-9_]*$" code))
               (raise-ail "RUNTIME_INVALID_API_ERROR"
                          "api-error code must be a bounded uppercase identifier"))
             (unless (and (string? message)
                          (<= 1 (string-length message) 512))
               (raise-ail "RUNTIME_INVALID_API_ERROR"
                          "api-error message must be a non-empty bounded string"))
             (define details
               (if (= (length arguments) 4) (cadddr arguments) (hash)))
             (hash "status" status
                   "headers" (hash)
                   "body"
                   (hash "error"
                         (hash "code" code
                               "message" message
                               "details" details)))))
  base)

(define maximum-library-result-nodes 10000)
(define maximum-library-result-depth 64)
(define maximum-library-result-string-length (* 1024 1024))

(define (install-libraries! target requirements context backends)
  (unless (hash? backends)
    (raise-argument-error 'execute-export "hash?" backends))
  (for ([requirement (in-list requirements)])
    (define library-name (library-requirement-name requirement))
    (define library-version (library-requirement-version requirement))
    (define contract (find-library-contract library-name library-version))
    (unless contract
      (raise-ail "RUNTIME_LIBRARY_CONTRACT_MISSING"
                 "parsed program refers to an unknown library contract"
                 (hasheq 'library (symbol->string library-name)
                         'version library-version)))
    (define backend
      (hash-ref backends (cons library-name library-version) #f))
    (unless backend
      (raise-ail "RUNTIME_LIBRARY_UNAVAILABLE"
                 "host did not provide a declared library backend"
                 (hasheq 'library (symbol->string library-name)
                         'version library-version)))
    (validate-library-backend! backend contract)
    (define provider (library-backend-provider backend))
    (define implementations (library-backend-implementations backend))
    (for ([(operation-name operation-contract)
           (in-hash (library-contract-operations contract))])
      (define implementation (hash-ref implementations operation-name))
      (environment-define!
       target
       operation-name
       (primitive
        operation-name
        (library-operation-contract-minimum-arity operation-contract)
        (library-operation-contract-maximum-arity operation-contract)
        (lambda (arguments runtime-context)
          (check-library-arguments! operation-contract arguments)
          (define cost
            (with-handlers
                ([exn:fail?
                  (lambda (_error)
                    (raise-ail
                     "RUNTIME_LIBRARY_CONTRACT_FAILURE"
                     "library cost estimator failed"
                     (library-call-details library-name
                                           library-version
                                           operation-name
                                           provider)))])
              ((library-operation-contract-cost operation-contract)
               arguments)))
          (unless (exact-nonnegative-integer? cost)
            (raise-ail
             "RUNTIME_LIBRARY_CONTRACT_FAILURE"
             "library cost estimator returned an invalid cost"
             (library-call-details library-name
                                   library-version
                                   operation-name
                                   provider)))
          (consume-fuel-amount! runtime-context cost)
          (define raw-result
            (with-handlers
                ([exn:fail?
                  (lambda (_error)
                    (raise-ail
                     "RUNTIME_LIBRARY_FAILURE"
                     "library backend operation failed"
                     (library-call-details library-name
                                           library-version
                                           operation-name
                                           provider)))])
              (implementation arguments)))
          (define result
            (normalize-library-result raw-result
                                      runtime-context
                                      library-name
                                      library-version
                                      operation-name
                                      provider))
          (unless (library-kind-matches?
                   (library-operation-contract-result-kind operation-contract)
                   result)
            (raise-ail
             "RUNTIME_LIBRARY_INVALID_RESULT"
             "library backend returned a value of the wrong kind"
             (hash-set
              (hash-set
               (library-call-details library-name
                                     library-version
                                     operation-name
                                     provider)
               'expected
               (symbol->string
                (library-operation-contract-result-kind operation-contract)))
              'actual
              (value-kind result))))
          result))))))

(define (validate-library-backend! backend contract)
  (define library-name (library-contract-name contract))
  (define library-version (library-contract-version contract))
  (unless (library-backend? backend)
    (raise-invalid-library-backend library-name
                                   library-version
                                   "registry entry is not a library backend"))
  (unless (and (eq? (library-backend-name backend) library-name)
               (exact-positive-integer? (library-backend-version backend))
               (= (library-backend-version backend) library-version))
    (raise-invalid-library-backend library-name
                                   library-version
                                   "backend identity does not match registry key"))
  (define provider (library-backend-provider backend))
  (unless (and (string? provider)
               (<= 1 (string-length provider) 128))
    (raise-invalid-library-backend library-name
                                   library-version
                                   "backend provider label is invalid"))
  (define implementations (library-backend-implementations backend))
  (unless (hash? implementations)
    (raise-invalid-library-backend library-name
                                   library-version
                                   "backend implementations are not a map"))
  (define expected-names
    (sort (hash-keys (library-contract-operations contract)) symbol<?))
  (define actual-names
    (and (andmap symbol? (hash-keys implementations))
         (sort (hash-keys implementations) symbol<?)))
  (unless (and actual-names (equal? actual-names expected-names))
    (raise-invalid-library-backend
     library-name
     library-version
     "backend functions do not exactly match the contract"
     (hasheq 'expected (map symbol->string expected-names)
             'actual (if actual-names
                         (map symbol->string actual-names)
                         '()))))
  (for ([(name implementation) (in-hash implementations)])
    (unless (procedure? implementation)
      (raise-invalid-library-backend
       library-name
       library-version
       "backend function is not callable"
       (hasheq 'operation (symbol->string name))))))

(define (raise-invalid-library-backend library-name
                                       library-version
                                       message
                                       [extra-details (hasheq)])
  (raise-ail
   "RUNTIME_INVALID_LIBRARY_BACKEND"
   message
   (for/fold ([details
               (hasheq 'library (symbol->string library-name)
                       'version library-version)])
             ([(key value) (in-hash extra-details)])
     (hash-set details key value))))

(define (check-library-arguments! contract arguments)
  (for ([expected (in-list (library-operation-contract-argument-kinds contract))]
        [argument (in-list arguments)]
        [index (in-naturals)])
    (unless (library-kind-matches? expected argument)
      (raise-ail
       "RUNTIME_TYPE"
       "library function received a value of the wrong type"
       (hasheq 'operation
               (symbol->string (library-operation-contract-name contract))
               'index index
               'expected (symbol->string expected)
               'actual (value-kind argument))))))

(define (library-kind-matches? expected value)
  (case expected
    [(Any Data) #t]
    [(Nil) (null? value)]
    [(Bool) (boolean? value)]
    [(Int) (exact-integer? value)]
    [(String) (string? value)]
    [(Symbol) (symbol? value)]
    [(List) (list? value)]
    [(Map) (hash? value)]
    [(Result) (or (ok-value? value) (err-value? value))]
    [else #f]))

(define (normalize-library-result value
                                  context
                                  library-name
                                  library-version
                                  operation-name
                                  provider)
  (define node-count 0)
  (define (invalid message [extra-details (hasheq)])
    (raise-ail
     "RUNTIME_LIBRARY_INVALID_RESULT"
     message
     (for/fold ([details
                 (library-call-details library-name
                                       library-version
                                       operation-name
                                       provider)])
               ([(key item) (in-hash extra-details)])
       (hash-set details key item))))
  (define (visit item depth)
    (set! node-count (add1 node-count))
    (consume-fuel! context)
    (when (> node-count maximum-library-result-nodes)
      (invalid "library backend result exceeds the node limit"
               (hasheq 'maximum maximum-library-result-nodes)))
    (when (> depth maximum-library-result-depth)
      (invalid "library backend result exceeds the depth limit"
               (hasheq 'maximum maximum-library-result-depth)))
    (cond
      [(or (exact-integer? item) (boolean? item) (symbol? item) (null? item))
       item]
      [(string? item)
       (define length (string-length item))
       (when (> length maximum-library-result-string-length)
         (invalid "library backend result contains an oversized string"
                  (hasheq 'maximum maximum-library-result-string-length)))
       (consume-fuel-amount! context (quotient (+ length 63) 64))
       (string->immutable-string item)]
      [(list? item)
       (for/list ([child (in-list item)])
         (visit child (add1 depth)))]
      [(hash? item)
       (for/hash ([(key child) (in-hash item)])
         (unless (or (symbol? key) (string? key))
           (invalid "library backend result contains an invalid map key"
                    (hasheq 'kind (value-kind key))))
         (define normalized-key
           (if (string? key) (string->immutable-string key) key))
         (values normalized-key (visit child (add1 depth))))]
      [(ok-value? item) (ok-value (visit (ok-value-value item) (add1 depth)))]
      [(err-value? item) (err-value (visit (err-value-value item) (add1 depth)))]
      [else
       (invalid "library backend result is not portable guest data"
                (hasheq 'kind (value-kind item)))]))
  (visit value 0))

(define (library-call-details library-name
                              library-version
                              operation-name
                              provider)
  (hasheq 'library (symbol->string library-name)
          'version library-version
          'operation (symbol->string operation-name)
          'provider provider))

(define (install-capabilities! target capabilities context bindings)
  (unless (hash? bindings)
    (raise-argument-error 'execute-export "hash?" bindings))
  (for ([capability (in-list capabilities)])
    (case capability
      [(log)
       (environment-define!
        target
        'log
        (primitive
         'log
         1
         1
         (lambda (arguments runtime-context)
           ((execution-context-logger runtime-context) (car arguments))
           '())))]
      [else
       (define primitives (hash-ref bindings capability #f))
       (unless (hash? primitives)
         (raise-ail "RUNTIME_CAPABILITY_UNAVAILABLE"
                    "host did not provide a declared capability"
                    (hasheq 'capability (symbol->string capability))))
       (for ([(name specification) (in-hash primitives)])
         (unless (and (symbol? name) (capability-primitive? specification))
           (raise-ail "RUNTIME_INVALID_CAPABILITY_BINDING"
                      "host capability binding is malformed"
                      (hasheq 'capability (symbol->string capability))))
         (define minimum (capability-primitive-minimum-arity specification))
         (define maximum (capability-primitive-maximum-arity specification))
         (define implementation
           (capability-primitive-implementation specification))
         (unless (and (exact-nonnegative-integer? minimum)
                      (or (not maximum)
                          (and (exact-nonnegative-integer? maximum)
                               (>= maximum minimum)))
                      (procedure? implementation))
           (raise-ail "RUNTIME_INVALID_CAPABILITY_BINDING"
                      "host capability primitive is malformed"
                      (hasheq 'capability (symbol->string capability)
                              'primitive (symbol->string name))))
         (environment-define!
          target
          name
          (primitive
           name
           minimum
           maximum
           (lambda (arguments _runtime-context)
             (implementation arguments)))))])))

(define (environment-define! target name value)
  (hash-set! (environment-bindings target) name value))

(define (environment-lookup target name)
  (cond
    [(hash-has-key? (environment-bindings target) name)
     (hash-ref (environment-bindings target) name)]
    [(environment-parent target)
     (environment-lookup (environment-parent target) name)]
    [else
     (raise-ail "RUNTIME_UNBOUND_NAME"
                "name is not bound in the current environment"
                (hasheq 'name (symbol->string name)))]))

(define (consume-fuel! context)
  (consume-fuel-amount! context 1))

(define (consume-fuel-amount! context amount)
  (define remaining (execution-context-fuel context))
  (when (< remaining amount)
    (raise-ail "RUNTIME_FUEL_EXHAUSTED"
               "execution exhausted its fuel allowance"))
  (set-execution-context-fuel! context (- remaining amount)))

(define (check-depth! context depth)
  (when (> depth (execution-context-maximum-depth context))
    (raise-ail "RUNTIME_DEPTH_EXHAUSTED"
               "execution exceeded its maximum call depth"
               (hasheq 'maxDepth
                       (execution-context-maximum-depth context)))))

(define (check-primitive-arity! callable arguments)
  (define actual (length arguments))
  (define minimum (primitive-minimum-arity callable))
  (define maximum (primitive-maximum-arity callable))
  (unless (and (>= actual minimum)
               (or (not maximum) (<= actual maximum)))
    (raise-arity-error (symbol->string (primitive-name callable))
                       minimum
                       maximum
                       actual)))

(define (raise-arity-error name minimum maximum actual)
  (raise-ail "RUNTIME_ARITY"
             "callable received the wrong number of arguments"
             (hasheq 'name name
                     'minimum minimum
                     'maximum (if maximum maximum "unbounded")
                     'actual actual)))

(define (expect-integer operation value)
  (unless (exact-integer? value)
    (raise-type-error operation "Int" value))
  value)

(define (expect-list operation value)
  (unless (list? value)
    (raise-type-error operation "List" value))
  value)

(define (expect-map operation value)
  (unless (hash? value)
    (raise-type-error operation "Map" value))
  value)

(define (expect-map-key operation value)
  (unless (or (symbol? value) (string? value))
    (raise-type-error operation "String or Symbol key" value))
  value)

(define (expect-http-status operation value minimum)
  (unless (and (exact-integer? value) (<= minimum value 599))
    (raise-ail "RUNTIME_INVALID_HTTP_STATUS"
               "HTTP response status is outside the allowed range"
               (hasheq 'operation (symbol->string operation)
                       'minimum minimum
                       'actual (if (exact-integer? value)
                                   value
                                   (value-kind value)))))
  value)

(define (raise-type-error operation expected value)
  (raise-ail "RUNTIME_TYPE"
             "primitive received a value of the wrong type"
             (hasheq 'operation (if (symbol? operation)
                                    (symbol->string operation)
                                    operation)
                     'expected expected
                     'actual (value-kind value))))

(define (value-kind value)
  (cond
    [(null? value) "Nil"]
    [(boolean? value) "Bool"]
    [(exact-integer? value) "Int"]
    [(string? value) "String"]
    [(symbol? value) "Symbol"]
    [(list? value) "List"]
    [(hash? value) "Map"]
    [(ok-value? value) "Ok"]
    [(err-value? value) "Err"]
    [(schema-value? value) "Schema"]
    [(closure? value) "Function"]
    [(primitive? value) "Primitive"]
    [else "Unknown"] ))

(define (value->jsexpr value)
  (cond
    [(or (exact-integer? value) (boolean? value) (string? value)) value]
    [(null? value) '()]
    [(symbol? value) (hasheq '$symbol (symbol->string value))]
    [(list? value) (map value->jsexpr value)]
    [(hash? value)
     (for/hasheq ([(key item) (in-hash value)])
       (values (cond
                 [(symbol? key) key]
                 [(string? key) (string->symbol key)]
                 [else
                  (raise-ail "RUNTIME_UNSERIALIZABLE_KEY"
                             "map key cannot be encoded as JSON"
                             (hasheq 'kind (value-kind key)))])
               (value->jsexpr item)))]
    [(ok-value? value)
     (hasheq 'ok (value->jsexpr (ok-value-value value)))]
    [(err-value? value)
     (hasheq 'error (value->jsexpr (err-value-value value)))]
    [else
     (raise-ail "RUNTIME_UNSERIALIZABLE_VALUE"
                "runtime value cannot be encoded as JSON"
                (hasheq 'kind (value-kind value)))]))

(define (jsexpr->value value)
  (cond
    [(eq? value (json-null)) '()]
    [(or (exact-integer? value) (boolean? value) (string? value)) value]
    [(list? value) (map jsexpr->value value)]
    [(hash? value)
     (for/hash ([(key item) (in-hash value)])
       (values (cond
                 [(symbol? key) (symbol->string key)]
                 [(string? key) key]
                 [else
                  (raise-ail "INPUT_UNSUPPORTED_JSON_KEY"
                             "JSON object key cannot be converted to a guest string"
                             (hasheq 'key (format "~s" key)))])
               (jsexpr->value item)))]
    [else
     (raise-ail "INPUT_UNSUPPORTED_JSON"
                "JSON input cannot be converted to a guest value"
                (hasheq 'value (format "~s" value)))]))

(define (default-logger value)
  (displayln (format "[guest] ~s" value)))
