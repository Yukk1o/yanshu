#lang racket/base

(provide (struct-out ail-program)
         (struct-out ail-route)
         (struct-out ail-schema)
         (struct-out library-requirement)
         (struct-out schema-any)
         (struct-out schema-string)
         (struct-out schema-integer)
         (struct-out schema-boolean)
         (struct-out schema-list)
         (struct-out schema-object)
         (struct-out schema-field)
         (struct-out ail-definition)
         (struct-out ast-binding)
         (struct-out expr-lit)
         (struct-out expr-var)
         (struct-out expr-quote)
         (struct-out expr-if)
         (struct-out expr-let)
         (struct-out expr-fn)
         (struct-out expr-do)
         (struct-out expr-call)
         datum->portable-jsexpr
         schema->jsexpr
         ast->jsexpr
         program->jsexpr)

(require "library-contract.rkt")

(struct ail-program
  (name version capabilities libraries schemas routes definitions exports source)
  #:transparent)
(struct ail-route (method path handler) #:transparent)
(struct ail-schema (name specification) #:transparent)
(struct schema-any () #:transparent)
(struct schema-string (minimum-length maximum-length) #:transparent)
(struct schema-integer (minimum maximum) #:transparent)
(struct schema-boolean () #:transparent)
(struct schema-list (item minimum-length maximum-length) #:transparent)
(struct schema-object (fields) #:transparent)
(struct schema-field
  (name specification required? has-default? default)
  #:transparent)
(struct ail-definition (name expression) #:transparent)
(struct ast-binding (name expression) #:transparent)

(struct expr-lit (value) #:transparent)
(struct expr-var (name) #:transparent)
(struct expr-quote (datum) #:transparent)
(struct expr-if (condition consequent alternative) #:transparent)
(struct expr-let (bindings body) #:transparent)
(struct expr-fn (parameters body) #:transparent)
(struct expr-do (expressions) #:transparent)
(struct expr-call (callee arguments) #:transparent)

(define (datum->portable-jsexpr value)
  (cond
    [(or (exact-integer? value) (boolean? value) (string? value)) value]
    [(symbol? value) (hasheq '$symbol (symbol->string value))]
    [(null? value) '()]
    [(list? value) (map datum->portable-jsexpr value)]
    [else (hasheq '$unsupported (format "~s" value))]))

(define (ast->jsexpr expression)
  (cond
    [(expr-lit? expression)
     (hasheq 'type "literal"
             'value (datum->portable-jsexpr (expr-lit-value expression)))]
    [(expr-var? expression)
     (hasheq 'type "variable"
             'name (symbol->string (expr-var-name expression)))]
    [(expr-quote? expression)
     (hasheq 'type "quote"
             'datum (datum->portable-jsexpr (expr-quote-datum expression)))]
    [(expr-if? expression)
     (hasheq 'type "if"
             'condition (ast->jsexpr (expr-if-condition expression))
             'consequent (ast->jsexpr (expr-if-consequent expression))
             'alternative (ast->jsexpr (expr-if-alternative expression)))]
    [(expr-let? expression)
     (hasheq
      'type "let"
      'bindings
      (for/list ([binding (in-list (expr-let-bindings expression))])
        (hasheq 'name (symbol->string (ast-binding-name binding))
                'expression (ast->jsexpr (ast-binding-expression binding))))
      'body (ast->jsexpr (expr-let-body expression)))]
    [(expr-fn? expression)
     (hasheq 'type "function"
             'parameters (map symbol->string (expr-fn-parameters expression))
             'body (ast->jsexpr (expr-fn-body expression)))]
    [(expr-do? expression)
     (hasheq 'type "do"
             'expressions (map ast->jsexpr (expr-do-expressions expression)))]
    [(expr-call? expression)
     (hasheq 'type "call"
             'callee (ast->jsexpr (expr-call-callee expression))
             'arguments (map ast->jsexpr (expr-call-arguments expression)))]
    [else
     (error 'ast->jsexpr "unknown AST node: ~s" expression)]))

(define (schema->jsexpr specification)
  (cond
    [(schema-any? specification) (hasheq 'type "any")]
    [(schema-string? specification)
     (hasheq 'type "string"
             'minimumLength (schema-string-minimum-length specification)
             'maximumLength (or (schema-string-maximum-length specification) #f))]
    [(schema-integer? specification)
     (hasheq 'type "integer"
             'minimum (or (schema-integer-minimum specification) #f)
             'maximum (or (schema-integer-maximum specification) #f))]
    [(schema-boolean? specification) (hasheq 'type "boolean")]
    [(schema-list? specification)
     (hasheq 'type "list"
             'item (schema->jsexpr (schema-list-item specification))
             'minimumLength (schema-list-minimum-length specification)
             'maximumLength (schema-list-maximum-length specification))]
    [(schema-object? specification)
     (hasheq
      'type "object"
      'additionalProperties #f
      'fields
      (for/list ([field (in-list (schema-object-fields specification))])
        (define document
          (hasheq 'name (schema-field-name field)
                  'required (schema-field-required? field)
                  'schema (schema->jsexpr
                           (schema-field-specification field))))
        (if (schema-field-has-default? field)
            (hash-set document
                      'default
                      (datum->portable-jsexpr (schema-field-default field)))
            document)))]
    [else (error 'schema->jsexpr "unknown schema node: ~s" specification)]))

(define (program->jsexpr program)
  (hasheq
   'type "program"
   'name (symbol->string (ail-program-name program))
   'version (ail-program-version program)
   'capabilities (map symbol->string (ail-program-capabilities program))
   'libraries
   (for/list ([requirement (in-list (ail-program-libraries program))])
     (hasheq 'name (symbol->string (library-requirement-name requirement))
             'version (library-requirement-version requirement)))
   'schemas
   (for/list ([schema (in-list (ail-program-schemas program))])
     (hasheq 'name (symbol->string (ail-schema-name schema))
             'schema (schema->jsexpr (ail-schema-specification schema))))
   'routes
   (for/list ([route (in-list (ail-program-routes program))])
     (hasheq 'method (ail-route-method route)
             'path (ail-route-path route)
             'handler (symbol->string (ail-route-handler route))))
   'definitions
   (for/list ([definition (in-list (ail-program-definitions program))])
     (hasheq 'name (symbol->string (ail-definition-name definition))
             'expression
             (ast->jsexpr (ail-definition-expression definition))))
   'exports (map symbol->string (ail-program-exports program))))
