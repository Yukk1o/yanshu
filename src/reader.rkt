#lang racket/base

(require racket/port
         "error.rkt")

(provide read-source)

(define (read-source source
                     #:max-nodes [max-nodes 10000]
                     #:max-depth [max-depth 128])
  (define input (open-input-string source))
  (define datum
    (with-handlers ([exn:fail:read?
                     (lambda (error)
                       (raise-ail "READ_SYNTAX"
                                  (exn-message error)))])
      (parameterize ([read-accept-reader #f]
                     [read-accept-lang #f]
                     [read-accept-compiled #f]
                     [read-accept-graph #f])
        (read input))))
  (when (eof-object? datum)
    (raise-ail "READ_EMPTY" "source document is empty"))
  (define trailing
    (with-handlers ([exn:fail:read?
                     (lambda (error)
                       (raise-ail "READ_SYNTAX"
                                  (exn-message error)))])
      (read input)))
  (unless (eof-object? trailing)
    (raise-ail "READ_MULTIPLE_FORMS"
               "source document must contain exactly one top-level form"))
  (validate-datum datum max-nodes max-depth)
  datum)

(define (validate-datum root max-nodes max-depth)
  (define count 0)
  (define (visit value depth)
    (set! count (add1 count))
    (when (> count max-nodes)
      (raise-ail "READ_NODE_LIMIT"
                 "source exceeds the configured node limit"
                 (hasheq 'maxNodes max-nodes)))
    (when (> depth max-depth)
      (raise-ail "READ_DEPTH_LIMIT"
                 "source exceeds the configured nesting limit"
                 (hasheq 'maxDepth max-depth)))
    (cond
      [(or (exact-integer? value)
           (boolean? value)
           (string? value)
           (symbol? value)
           (null? value))
       (void)]
      [(list? value)
       (for ([item (in-list value)])
         (visit item (add1 depth)))]
      [else
       (raise-ail "READ_UNSUPPORTED_DATUM"
                  "source contains an unsupported datum"
                  (hasheq 'datum (format "~s" value)))])
    (void))
  (visit root 0))

