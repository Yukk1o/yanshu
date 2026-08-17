#lang racket/base

(require "evolution-loop.rkt"
         "runtime.rkt"
         "service-test-suite.rkt"
         "version-store.rkt")

(provide deploy-service!
         make-active-program-loader
         evolve-active-service-once)

(define (deploy-service! source suite store-root
                         #:provider [provider "manual-deploy"])
  (define program (load-program-source source))
  (define report (run-service-test-suite program suite))
  (define candidate-hash (source-hash source))
  (define current-hash (active-hash store-root))
  (cond
    [(equal? candidate-hash current-hash)
     (hasheq 'ok (hash-ref report 'passed)
             'store (path->string store-root)
             'candidate candidate-hash
             'report report
             'alreadyActive #t
             'promoted #f
             'active current-hash)]
    [else
     (register-candidate! store-root
                          source
                          #:parent current-hash
                          #:provider provider
                          #:report report)
     (define promoted? #f)
     (when (hash-ref report 'passed)
       (promote! store-root candidate-hash)
       (set! promoted? #t))
     (hasheq 'ok (hash-ref report 'passed)
             'store (path->string store-root)
             'candidate candidate-hash
             'report report
             'alreadyActive #f
             'promoted promoted?
             'active (active-hash store-root))]))

(define (make-active-program-loader store-root)
  (lambda ()
    ;; Resolve and parse once per request. The HTTP host keeps the returned
    ;; program pinned even if another request promotes a new active version.
    (load-program-source (active-source store-root))))

(define (evolve-active-service-once store-root suite provider
                                    #:promote? [promote? #f])
  (evolve-once (active-source store-root)
               suite
               provider
               store-root
               #:promote? promote?
               #:run-suite run-service-test-suite))
