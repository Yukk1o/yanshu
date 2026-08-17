#lang racket/base

(provide (struct-out ail-program)
         (struct-out ail-route)
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
         ast->jsexpr
         program->jsexpr)

(struct ail-program (name version capabilities routes definitions exports source)
  #:transparent)
(struct ail-route (method path handler) #:transparent)
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

(define (program->jsexpr program)
  (hasheq
   'type "program"
   'name (symbol->string (ail-program-name program))
   'version (ail-program-version program)
   'capabilities (map symbol->string (ail-program-capabilities program))
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
