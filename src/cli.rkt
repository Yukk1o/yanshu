#lang racket/base

(require json
         racket/file
         racket/path
         racket/runtime-path
         "ast.rkt"
         "error.rkt"
         "evolution-loop.rkt"
         "evolver.rkt"
         "http-server.rkt"
         "kv-store.rkt"
         "runtime.rkt"
         "service-deployment.rkt"
         "service-test-suite.rkt"
         "test-suite.rkt"
         "version-store.rkt")

(define-runtime-path source-directory ".")
(define project-root (simplify-path (build-path source-directory 'up)))

(define (emit value)
  (write-json value)
  (newline))

(define (main arguments)
  (cond
    [(equal? arguments '("demo")) (run-demo)]
    [(and (= (length arguments) 2)
          (member (car arguments) '("check" "inspect")))
     (define program (load-program-file (cadr arguments)))
     (emit (hasheq 'ok #t 'program (program->jsexpr program)))]
    [(and (= (length arguments) 3)
          (string=? (car arguments) "test"))
     (define program (load-program-file (cadr arguments)))
     (define suite (load-test-suite (caddr arguments)))
     (define report (run-test-suite program suite))
     (emit (hasheq 'ok (hash-ref report 'passed) 'report report))
     (unless (hash-ref report 'passed) (exit 1))]
    [(and (= (length arguments) 3)
          (string=? (car arguments) "test-service"))
     (define program (load-program-file (cadr arguments)))
     (define suite (load-service-test-suite (caddr arguments)))
     (define report (run-service-test-suite program suite))
     (emit (hasheq 'ok (hash-ref report 'passed) 'report report))
     (unless (hash-ref report 'passed) (exit 1))]
    [(and (= (length arguments) 4)
          (string=? (car arguments) "run"))
     (define program (load-program-file (cadr arguments)))
     (define entry (string->symbol (caddr arguments)))
     (define argument-input (cadddr arguments))
     (define argument-document
       (if (file-exists? argument-input)
           (call-with-input-file argument-input read-json)
           (string->jsexpr argument-input)))
     (unless (list? argument-document)
       (raise-ail "INPUT_ARGUMENTS_NOT_ARRAY"
                  "run arguments must be a JSON array"))
     (define result
       (execute-export program
                       entry
                       (map jsexpr->value argument-document)))
     (emit (hasheq 'ok #t 'result (value->jsexpr result)))]
    [(and (member (length arguments) '(3 4))
          (string=? (car arguments) "evolve"))
     (when (and (= (length arguments) 4)
                (not (string=? (list-ref arguments 3) "--promote")))
       (raise-ail "CLI_INVALID_OPTION"
                  "the only evolve option is --promote"))
     (define program-path (cadr arguments))
     (define tests-path (caddr arguments))
     (define current-source (file->string program-path))
     (define suite (load-test-suite tests-path))
     (define configured-store (getenv "AI_EVOLVE_STORE"))
     (define store-root
       (if (and configured-store
                (not (string=? configured-store "")))
           (string->path configured-store)
           (build-path project-root
                       ".runtime"
                       (string-append
                        "live-"
                        (substring (source-hash current-source) 0 16)))))
     (define result
       (evolve-once current-source
                    suite
                    (make-configured-provider)
                    store-root
                    #:promote? (= (length arguments) 4)))
     (emit result)
     (unless (hash-ref result 'ok) (exit 1))]
    [(and (= (length arguments) 4)
          (string=? (car arguments) "serve"))
     (serve-program (cadr arguments)
                    (caddr arguments)
                    (cadddr arguments))]
    [(and (= (length arguments) 4)
          (string=? (car arguments) "deploy-service"))
     (define source (file->string (cadr arguments)))
     (define suite (load-service-test-suite (caddr arguments)))
     (define result
       (deploy-service! source suite (string->path (cadddr arguments))))
     (emit result)
     (unless (hash-ref result 'ok) (exit 1))]
    [(and (= (length arguments) 4)
          (string=? (car arguments) "serve-active"))
     (define code-store (string->path (cadr arguments)))
     (serve-loader (make-active-program-loader code-store)
                   (format "active:~a" (path->string code-store))
                   (caddr arguments)
                   (cadddr arguments))]
    [(and (member (length arguments) '(3 4))
          (string=? (car arguments) "evolve-service"))
     (when (and (= (length arguments) 4)
                (not (string=? (list-ref arguments 3) "--promote")))
       (raise-ail "CLI_INVALID_OPTION"
                  "the only evolve-service option is --promote"))
     (define code-store (string->path (cadr arguments)))
     (define suite (load-service-test-suite (caddr arguments)))
     (define result
       (evolve-active-service-once
        code-store
        suite
        (make-configured-provider)
        #:promote? (= (length arguments) 4)))
     (emit result)
     (unless (hash-ref result 'ok) (exit 1))]
    [(and (= (length arguments) 2)
          (string=? (car arguments) "rollback-service"))
     (define code-store (string->path (cadr arguments)))
     (define rolled-back (rollback! code-store))
     (emit (hasheq 'ok #t
                   'store (path->string code-store)
                   'active rolled-back))]
    [else
     (emit-usage)
     (exit 2)]))

(define (serve-program program-path port-text store-path)
  (load-program-file program-path)
  (serve-loader (lambda () (load-program-file program-path))
                program-path
                port-text
                store-path))

(define (serve-loader program-loader program-label port-text store-path)
  (define port (string->number port-text))
  (unless (and (exact-integer? port) (<= 0 port 65535))
    (raise-ail "CLI_INVALID_PORT"
               "serve port must be an integer from 0 through 65535"))
  (program-loader)
  (define store (open-file-kv-store store-path))
  (define observation-path
    (string->path (string-append store-path ".observations.jsonl")))
  (define observation-lock (make-semaphore 1))
  (define (record-observation observation)
    (semaphore-wait observation-lock)
    (dynamic-wind
      void
      (lambda ()
        (define parent (path-only observation-path))
        (when parent (make-directory* parent))
        (call-with-output-file observation-path
          (lambda (output)
            (write-json observation output)
            (newline output))
          #:exists 'append))
      (lambda () (semaphore-post observation-lock))))
  (define server
    (start-http-server
     program-loader
     #:store store
     #:port port
     #:static-root (build-path project-root "web" "tasks")
     #:logger (lambda (value) (eprintf "[guest] ~s\n" value))
     #:observer record-observation))
  (emit
   (hasheq 'ok #t
           'event "listening"
           'host (running-http-server-host server)
           'port (running-http-server-port server)
           'program program-label
           'store store-path
           'observations (path->string observation-path)))
  (dynamic-wind
    void
    (lambda ()
      (with-handlers ([exn:break? (lambda (_error) (void))])
        (sync never-evt)))
    (lambda () (stop-http-server! server))))

(define (run-demo)
  (define example-root (build-path project-root "examples" "discount"))
  (define initial-path (build-path example-root "v1.ail"))
  (define candidate-path (build-path example-root "v2.ail"))
  (define test-path (build-path example-root "tests.json"))
  (define store-root
    (build-path project-root
                ".runtime"
                (format "demo-~a-~a" (current-seconds) (random 1000000))))

  (define initial-source (file->string initial-path))
  (define initial-program (load-program-source initial-source))
  (define suite (load-test-suite test-path))
  (define bootstrap-report
    (hasheq 'passed #t
            'total 0
            'passedCount 0
            'failedCount 0
            'failures '()
            'reason "trusted bootstrap"))
  (define initial-hash
    (register-candidate! store-root
                         initial-source
                         #:provider "bootstrap"
                         #:report bootstrap-report))
  (promote! store-root initial-hash)

  (define initial-report (run-test-suite initial-program suite))
  (when (hash-ref initial-report 'passed)
    (raise-ail "DEMO_INITIAL_VERSION_UNEXPECTEDLY_PASSED"
               "demo requires the initial version to expose a failing case"))

  (define provider (make-file-provider candidate-path))
  (define request
    (evolution-request initial-hash initial-source initial-report))
  (define proposal (request-proposal provider request))
  (define candidate-source (evolution-proposal-source proposal))
  (define candidate-program (load-program-source candidate-source))
  (define candidate-report (run-test-suite candidate-program suite))
  (unless (hash-ref candidate-report 'passed)
    (raise-ail "DEMO_CANDIDATE_FAILED"
               "offline provider candidate did not pass the complete suite"
               (hasheq 'report candidate-report)))

  (define candidate-hash
    (register-candidate!
     store-root
     candidate-source
     #:parent initial-hash
     #:provider (evolution-proposal-provider proposal)
     #:provider-metadata (evolution-proposal-metadata proposal)
     #:report candidate-report))
  (promote! store-root candidate-hash)
  (define promoted-result
    (value->jsexpr
     (execute-export (load-program-source (active-source store-root))
                     'calculate-discount
                     (list 100 "vip"))))

  (define rollback-hash (rollback! store-root))
  (define rollback-result
    (value->jsexpr
     (execute-export (load-program-source (active-source store-root))
                     'calculate-discount
                     (list 100 "vip"))))

  (emit
   (hasheq
    'ok #t
    'store (path->string store-root)
    'initial
    (hasheq 'hash initial-hash
            'report initial-report)
    'candidate
    (hasheq 'hash candidate-hash
            'provider (evolution-proposal-provider proposal)
            'report candidate-report)
    'promotion
    (hasheq 'active candidate-hash
            'vipResult promoted-result)
    'rollback
    (hasheq 'active rollback-hash
            'vipResult rollback-result))))

(define (emit-usage)
  (emit
   (hasheq
    'ok #f
    'usage
    (list "demo"
          "check <program.ail>"
          "inspect <program.ail>"
          "test <program.ail> <tests.json>"
          "run <program.ail> <entry> <args-json-or-file>"
          "evolve <program.ail> <tests.json> [--promote]"
          "test-service <program.ail> <scenarios.json>"
          "deploy-service <program.ail> <scenarios.json> <code-store>"
          "serve <program.ail> <port> <data-store.json>"
          "serve-active <code-store> <port> <data-store.json>"
          "evolve-service <code-store> <scenarios.json> [--promote]"
          "rollback-service <code-store>"))))

(module+ main
  (with-handlers
      ([exn:fail:ail?
        (lambda (error)
          (emit (ail-error->jsexpr error))
          (exit 1))]
       [exn:fail?
        (lambda (error)
          (emit
           (hasheq 'ok #f
                   'error
                   (hasheq 'code "HOST_FAILURE"
                           'message (exn-message error)
                           'details (hasheq))))
          (exit 1))])
    (main (vector->list (current-command-line-arguments)))))
