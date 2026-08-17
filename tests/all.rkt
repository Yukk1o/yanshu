#lang racket/base

(require json
         racket/file
         racket/runtime-path
         racket/string
         "../src/ast.rkt"
         "../src/error.rkt"
         "../src/evolution-loop.rkt"
         "../src/evolver.rkt"
         "../src/reader.rkt"
         "../src/runtime.rkt"
         "../src/test-suite.rkt"
         "../src/version-store.rkt")

(define-runtime-path tests-directory ".")
(define project-root (simplify-path (build-path tests-directory 'up)))
(define example-root (build-path project-root "examples" "discount"))

(define total 0)
(define failed 0)

(define (test name thunk)
  (set! total (add1 total))
  (with-handlers
      ([exn:fail?
        (lambda (error)
          (set! failed (add1 failed))
          (displayln (format "not ok ~a - ~a" name (exn-message error))))])
    (thunk)
    (displayln (format "ok ~a - ~a" total name))))

(define (check-equal actual expected [label "values differ"])
  (unless (equal? actual expected)
    (error 'check-equal "~a: expected ~s, got ~s" label expected actual)))

(define (check-true actual [label "expected true"])
  (unless actual (error 'check-true "~a" label)))

(define (check-false actual [label "expected false"])
  (when actual (error 'check-false "~a" label)))

(define (check-ail-code expected thunk)
  (define observed #f)
  (with-handlers ([exn:fail:ail?
                   (lambda (error)
                     (set! observed (exn:fail:ail-code error)))])
    (thunk))
  (check-equal observed expected "diagnostic code differs"))

(define initial-source
  (file->string (build-path example-root "v1.ail")))
(define candidate-source
  (file->string (build-path example-root "v2.ail")))
(define initial-program (load-program-source initial-source))
(define candidate-program (load-program-source candidate-source))
(define discount-suite
  (load-test-suite (build-path example-root "tests.json")))

(test "reader rejects multiple top-level documents"
      (lambda ()
        (check-ail-code "READ_MULTIPLE_FORMS"
                        (lambda () (read-source "(program) (program)")))))

(test "candidate program executes through the independent interpreter"
      (lambda ()
        (check-equal
         (execute-export candidate-program
                         'calculate-discount
                         (list 100 "vip"))
         90)))

(test "recursive lexical closures work"
      (lambda ()
        (define factorial
          (load-program-source
           (string-append
            "(program (name factorial) (version 1) (capabilities) "
            "(def fact (fn (n) (if (<= n 1) 1 (* n (fact (- n 1)))))) "
            "(export fact))")))
        (check-equal (execute-export factorial 'fact (list 5)) 120)))

(test "let bindings are sequential"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name lets) (version 1) (capabilities) "
            "(def run (fn (x) (let ((a (+ x 1)) (b (+ a 1))) b))) "
            "(export run))")))
        (check-equal (execute-export program 'run (list 5)) 7)))

(test "undeclared capabilities are absent"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name no-log) (version 1) (capabilities) "
            "(def run (fn () (log \"hidden\"))) (export run))")))
        (check-ail-code
         "RUNTIME_UNBOUND_NAME"
         (lambda () (execute-export program 'run '())))))

(test "declared log capability is injected"
      (lambda ()
        (define observed '())
        (define program
          (load-program-source
           (string-append
            "(program (name with-log) (version 1) (capabilities log) "
            "(def run (fn (value) (do (log value) value))) (export run))")))
        (check-equal
         (execute-export
          program
          'run
          (list "hello")
          #:logger (lambda (value) (set! observed (cons value observed))))
         "hello")
        (check-equal observed '("hello"))))

(test "web routes are parsed into explicit program metadata"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name tasks) (version 1) (capabilities kv) "
            "(route GET \"/tasks/:id\" get-task) "
            "(def get-task (fn (request) (kv-get (get (get request \"params\") \"id\") #f))) "
            "(export get-task))")))
        (check-equal (length (ail-program-routes program)) 1)
        (define route (car (ail-program-routes program)))
        (check-equal (ail-route-method route) "GET")
        (check-equal (ail-route-path route) "/tasks/:id")
        (check-equal (ail-route-handler route) 'get-task)))

(test "ambiguous routes are rejected at parse time"
      (lambda ()
        (check-ail-code
         "PROGRAM_AMBIGUOUS_ROUTE"
         (lambda ()
           (load-program-source
            (string-append
             "(program (name bad-routes) (version 1) (capabilities) "
             "(route GET \"/items/:id\" first-handler) "
             "(route GET \"/items/:name\" second-handler) "
             "(def first-handler (fn (request) request)) "
             "(def second-handler (fn (request) request)) "
             "(export first-handler second-handler))"))))))

(test "declared host capabilities are injected without ambient authority"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name kv-reader) (version 1) (capabilities kv) "
            "(def read-value (fn (key) (kv-get key \"missing\"))) "
            "(export read-value))")))
        (check-ail-code
         "RUNTIME_CAPABILITY_UNAVAILABLE"
         (lambda () (execute-export program 'read-value (list "x"))))
        (define bindings
          (hasheq
           'kv
           (hasheq
            'kv-get
            (capability-primitive
             2
             2
             (lambda (arguments)
               (if (string=? (car arguments) "x")
                   42
                   (cadr arguments)))))))
        (check-equal
         (execute-export program
                         'read-value
                         (list "x")
                         #:capability-bindings bindings)
         42)))

(test "fuel stops infinite recursion"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name loop) (version 1) (capabilities) "
            "(def loop (fn () (loop))) (export loop))")))
        (check-ail-code
         "RUNTIME_FUEL_EXHAUSTED"
         (lambda ()
           (execute-export program 'loop '() #:fuel 20 #:max-depth 1000)))))

(test "regression suite distinguishes initial and candidate versions"
      (lambda ()
        (define initial-report (run-test-suite initial-program discount-suite))
        (define candidate-report (run-test-suite candidate-program discount-suite))
        (check-false (hash-ref initial-report 'passed))
        (check-equal (hash-ref initial-report 'failedCount) 2)
        (check-true (hash-ref candidate-report 'passed))))

(test "version store promotes and rolls back immutable versions"
      (lambda ()
        (define store (make-temporary-file "ai-evolve-store-~a" 'directory))
        (dynamic-wind
          void
          (lambda ()
            (define bootstrap-report (hasheq 'passed #t))
            (define initial-hash
              (register-candidate! store
                                   initial-source
                                   #:provider "test-bootstrap"
                                   #:report bootstrap-report))
            (promote! store initial-hash)
            (define candidate-report
              (run-test-suite candidate-program discount-suite))
            (define candidate-hash
              (register-candidate! store
                                   candidate-source
                                   #:parent initial-hash
                                   #:provider "test-provider"
                                   #:report candidate-report))
            (promote! store candidate-hash)
            (check-equal (active-hash store) candidate-hash)
            (check-equal (rollback! store) initial-hash)
            (check-equal (active-hash store) initial-hash))
          (lambda () (delete-directory/files store)))))

(test "failed reports cannot be promoted"
      (lambda ()
        (define store (make-temporary-file "ai-evolve-store-~a" 'directory))
        (dynamic-wind
          void
          (lambda ()
            (define hash
              (register-candidate! store
                                   initial-source
                                   #:provider "test-provider"
                                   #:report (hasheq 'passed #f)))
            (check-ail-code "VERSION_TESTS_NOT_PASSED"
                            (lambda () (promote! store hash))))
          (lambda () (delete-directory/files store)))))

(test "offline provider returns a complete candidate"
      (lambda ()
        (define provider
          (make-file-provider (build-path example-root "v2.ail")))
        (define proposal
          (request-proposal
           provider
           (evolution-request "initial" initial-source (hasheq))))
        (check-equal (evolution-proposal-source proposal) candidate-source)
        (check-equal (evolution-proposal-provider proposal) "offline-file")))

(define (completed-response source
                            #:id [id "resp_test_123"]
                            #:model [model "test-model"])
  (hasheq
   'id id
   'status "completed"
   'model model
   'usage (hasheq 'input_tokens 10 'output_tokens 20 'total_tokens 30)
   'output
   (list
    (hasheq
     'type "message"
     'content
     (list
      (hasheq
       'type "output_text"
       'text
       (jsexpr->string
        (hasheq 'source source
                'notes "repair VIP discount"))))))))

(test "Responses provider sends strict structured request and parses output"
      (lambda ()
        (define captured #f)
        (define (mock-transport endpoint headers body timeout)
          (set! captured
                (hasheq 'endpoint endpoint
                        'headers headers
                        'body body
                        'timeout timeout))
          (completed-response candidate-source))
        (define provider
          (make-openai-responses-provider
           #:api-key "test-secret-never-printed"
           #:base-url "https://provider.invalid/v1/"
           #:model "test-model"
           #:reasoning-effort "medium"
           #:max-output-tokens 4096
           #:timeout-seconds 17
           #:transport mock-transport))
        (define proposal
          (request-proposal
           provider
           (evolution-request "current-hash"
                              initial-source
                              (hasheq 'passed #f))))
        (check-equal (hash-ref captured 'endpoint)
                     "https://provider.invalid/v1/responses")
        (check-true
         (ormap (lambda (header)
                  (string-prefix? header "Authorization: Bearer "))
                (hash-ref captured 'headers)))
        (define body (hash-ref captured 'body))
        (check-false (hash-ref body 'store))
        (check-equal (hash-ref captured 'timeout) 17)
        (check-equal
         (hash-ref (hash-ref (hash-ref body 'text) 'format) 'type)
         "json_schema")
        (check-true
         (hash-ref (hash-ref (hash-ref body 'text) 'format) 'strict))
        (define input-document (string->jsexpr (hash-ref body 'input)))
        (check-equal (hash-ref input-document 'currentHash) "current-hash")
        (check-equal (evolution-proposal-source proposal) candidate-source)
        (check-equal
         (hash-ref (evolution-proposal-metadata proposal) 'responseId)
         "resp_test_123")))

(test "DeepSeek provider uses Chat Completions JSON mode and parses output"
      (lambda ()
        (define captured #f)
        (define provider
          (make-deepseek-chat-provider
           #:api-key "test-deepseek-key"
           #:base-url "https://api.deepseek.com/"
           #:model "deepseek-v4-flash"
           #:reasoning-effort "high"
           #:max-output-tokens 6000
           #:timeout-seconds 23
           #:transport
           (lambda (endpoint headers body timeout)
             (set! captured
                   (hasheq 'endpoint endpoint
                           'headers headers
                           'body body
                           'timeout timeout))
             (hasheq
              'id "chatcmpl_test_123"
              'model "deepseek-v4-flash"
              'usage (hasheq 'total_tokens 42)
              'choices
              (list
               (hasheq
                'finish_reason "stop"
                'message
                (hasheq
                 'role "assistant"
                 'content
                 (jsexpr->string
                  (hasheq 'source candidate-source
                          'notes "repair VIP discount")))))))))
        (define proposal
          (request-proposal
           provider
           (evolution-request "deepseek-current"
                              initial-source
                              (hasheq 'passed #f))))
        (check-equal (hash-ref captured 'endpoint)
                     "https://api.deepseek.com/chat/completions")
        (define body (hash-ref captured 'body))
        (check-equal (hash-ref body 'model) "deepseek-v4-flash")
        (check-equal (hash-ref body 'max_tokens) 6000)
        (check-equal (hash-ref (hash-ref body 'response_format) 'type)
                     "json_object")
        (check-equal (hash-ref (hash-ref body 'thinking) 'type) "enabled")
        (check-equal (length (hash-ref body 'messages)) 2)
        (check-equal (evolution-proposal-source proposal) candidate-source)
        (check-equal (evolution-proposal-provider proposal) "deepseek-chat")
        (check-equal
         (hash-ref (evolution-proposal-metadata proposal) 'responseId)
         "chatcmpl_test_123")))

(test "DeepSeek provider rejects truncated candidates"
      (lambda ()
        (define provider
          (make-deepseek-chat-provider
           #:api-key "test-key"
           #:transport
           (lambda (_endpoint _headers _body _timeout)
             (hasheq
              'id "chatcmpl_truncated"
              'choices
              (list
               (hasheq 'finish_reason "length"
                       'message (hasheq 'content "{}")))))))
        (check-ail-code
         "PROVIDER_INCOMPLETE_RESPONSE"
         (lambda ()
           (request-proposal
            provider
            (evolution-request "hash" initial-source (hasheq)))))))

(test "live provider requires a credential"
      (lambda ()
        (check-ail-code
         "PROVIDER_MISSING_API_KEY"
         (lambda ()
           (make-openai-responses-provider #:api-key #f)))))

(test "live provider reports refusals with a stable diagnostic"
      (lambda ()
        (define provider
          (make-openai-responses-provider
           #:api-key "test-key"
           #:transport
           (lambda (_endpoint _headers _body _timeout)
             (hasheq
              'id "resp_refusal"
              'status "completed"
              'output
              (list
               (hasheq 'type "message"
                       'content
                       (list (hasheq 'type "refusal"
                                     'refusal "cannot comply"))))))))
        (check-ail-code
         "PROVIDER_REFUSAL"
         (lambda ()
           (request-proposal
            provider
            (evolution-request "hash" initial-source (hasheq)))))))

(test "live provider rejects malformed structured output"
      (lambda ()
        (define provider
          (make-openai-responses-provider
           #:api-key "test-key"
           #:transport
           (lambda (_endpoint _headers _body _timeout)
             (hasheq
              'id "resp_bad_json"
              'status "completed"
              'output
              (list
               (hasheq 'type "message"
                       'content
                       (list (hasheq 'type "output_text"
                                     'text "not-json"))))))))
        (check-ail-code
         "PROVIDER_INVALID_CANDIDATE_JSON"
         (lambda ()
           (request-proposal
            provider
            (evolution-request "hash" initial-source (hasheq)))))))

(test "live evolution validates, records metadata, and explicitly promotes"
      (lambda ()
        (define store (make-temporary-file "ai-evolve-live-~a" 'directory))
        (dynamic-wind
          void
          (lambda ()
            (define provider
              (make-openai-responses-provider
               #:api-key "test-key"
               #:model "test-model"
               #:transport
               (lambda (_endpoint _headers _body _timeout)
                 (completed-response candidate-source))))
            (define result
              (evolve-once initial-source
                           discount-suite
                           provider
                           store
                           #:promote? #t))
            (check-true (hash-ref result 'ok))
            (check-true (hash-ref result 'promoted))
            (define candidate-hash
              (hash-ref (hash-ref result 'candidate) 'hash))
            (check-equal (active-hash store) candidate-hash)
            (define metadata (version-metadata store candidate-hash))
            (check-equal
             (hash-ref (hash-ref metadata 'providerMetadata) 'responseId)
             "resp_test_123"))
          (lambda () (delete-directory/files store)))))

(test "failed live candidate remains inactive even when promotion is requested"
      (lambda ()
        (define store (make-temporary-file "ai-evolve-live-fail-~a" 'directory))
        (dynamic-wind
          void
          (lambda ()
            (define provider
              (evolution-provider
               "simulated-failure"
               (lambda (_request)
                 (evolution-proposal
                  (string-replace initial-source
                                  "(name discount)"
                                  "(name discount-broken)")
                  "simulated-failure"
                  "candidate intentionally keeps the bug"
                  (hasheq 'kind "test")))))
            (define result
              (evolve-once initial-source
                           discount-suite
                           provider
                           store
                           #:promote? #t))
            (check-false (hash-ref result 'ok))
            (check-false (hash-ref result 'promoted))
            (check-equal (active-hash store) (source-hash initial-source)))
          (lambda () (delete-directory/files store)))))

(test "passing live candidate is inactive without explicit promotion"
      (lambda ()
        (define store (make-temporary-file "ai-evolve-live-no-promote-~a" 'directory))
        (dynamic-wind
          void
          (lambda ()
            (define provider
              (evolution-provider
               "simulated-pass"
               (lambda (_request)
                 (evolution-proposal candidate-source
                                     "simulated-pass"
                                     "valid candidate"
                                     (hasheq 'kind "test")))))
            (define result
              (evolve-once initial-source discount-suite provider store))
            (check-true (hash-ref result 'ok))
            (check-false (hash-ref result 'promotionRequested))
            (check-false (hash-ref result 'promoted))
            (check-equal (active-hash store) (source-hash initial-source)))
          (lambda () (delete-directory/files store)))))

(displayln (format "1..~a" total))
(displayln (format "~a passed; ~a failed" (- total failed) failed))
(when (positive? failed) (exit 1))
