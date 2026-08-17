#lang racket/base

(require racket/file
         "error.rkt"
         "evolver.rkt"
         "runtime.rkt"
         "test-suite.rkt"
         "version-store.rkt")

(provide evolve-once)

(define (evolve-once current-source suite provider store-root
                     #:promote? [promote? #f]
                     #:run-suite [run-suite run-test-suite])
  (define current-program (load-program-source current-source))
  (define current-report (run-suite current-program suite))
  (define source-current-hash (source-hash current-source))
  (define stored-active-hash (active-hash store-root))
  (cond
    [(not stored-active-hash)
     (define bootstrap-hash
       (register-candidate!
        store-root
        current-source
        #:provider "bootstrap"
        #:report (hasheq 'passed #t
                         'reason "trusted operator-supplied bootstrap")))
     (promote! store-root bootstrap-hash)]
    [(not (equal? stored-active-hash source-current-hash))
     (raise-ail "EVOLVE_ACTIVE_SOURCE_MISMATCH"
                "the supplied source is not the active stored version"
                (hasheq 'supplied source-current-hash
                        'active stored-active-hash))])
  (define request
    (evolution-request source-current-hash current-source current-report))
  (define proposal (request-proposal provider request))
  (define candidate-source (evolution-proposal-source proposal))
  (define candidate-program (load-program-source candidate-source))
  (define candidate-report (run-suite candidate-program suite))
  (define candidate-hash
    (register-candidate!
     store-root
     candidate-source
     #:parent source-current-hash
     #:provider (evolution-proposal-provider proposal)
     #:provider-metadata (evolution-proposal-metadata proposal)
     #:report candidate-report))
  (define promoted? #f)
  (when (and promote? (hash-ref candidate-report 'passed))
    (promote! store-root candidate-hash)
    (set! promoted? #t))
  (hasheq
   'ok (hash-ref candidate-report 'passed)
   'store (path->string store-root)
   'current (hasheq 'hash source-current-hash
                    'report current-report)
   'candidate (hasheq 'hash candidate-hash
                      'provider (evolution-proposal-provider proposal)
                      'notes (evolution-proposal-notes proposal)
                      'report candidate-report)
   'promotionRequested promote?
   'promoted promoted?
   'active (active-hash store-root)))
