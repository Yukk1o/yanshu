#lang racket/base

(require json
         racket/file
         racket/list
         racket/port
         racket/runtime-path
         racket/string
         racket/tcp
         "../src/ast.rkt"
         "../src/conformance-suite.rkt"
         "../src/error.rkt"
         "../src/evolution-loop.rkt"
         "../src/evolver.rkt"
         "../src/http-server.rkt"
         "../src/kv-store.rkt"
         "../src/reader.rkt"
         "../src/runtime.rkt"
         "../src/service.rkt"
         "../src/service-deployment.rkt"
         "../src/service-test-suite.rkt"
         "../src/test-suite.rkt"
         "../src/version-store.rkt")

(define-runtime-path tests-directory ".")
(define project-root (simplify-path (build-path tests-directory 'up)))
(define example-root (build-path project-root "examples" "discount"))
(define library-example-root (build-path project-root "examples" "libraries"))
(define task-example-path
  (build-path project-root "examples" "tasks" "service.ail"))
(define conformance-manifest-path
  (build-path project-root "conformance" "v1" "manifest.json"))

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

(define (check-api-error-envelope body expected-code)
  (unless (and (hash? body) (hash? (hash-ref body 'error #f)))
    (error 'check-api-error-envelope "response has no error envelope: ~s" body))
  (define public-error (hash-ref body 'error))
  (check-equal (sort (hash-keys public-error) symbol<?)
               '(code details message)
               "error envelope fields differ")
  (check-equal (hash-ref public-error 'code) expected-code)
  (check-true (string? (hash-ref public-error 'message))
              "error message is not a string")
  (check-true (hash-has-key? public-error 'details)
              "error details are missing"))

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

(test "host-neutral conformance manifest matches the Racket oracle"
      (lambda ()
        (define report (run-conformance-manifest conformance-manifest-path))
        (check-true (hash-ref report 'passed)
                    (format "conformance failures: ~s"
                            (filter (lambda (case-result)
                                      (not (hash-ref case-result 'passed)))
                                    (hash-ref report 'cases))))
        (check-equal (hash-ref report 'total) 17)))

(test "conformance fixture codec preserves portable guest values"
      (lambda ()
        (define guest-value
          (hasheq 'integer 9223372036854775808
                  'nil '()
                  'symbol 'ready
                  'result (ok-value (list 1 #t "value"))))
        (check-equal (fixture->value (value->fixture guest-value))
                     (hash "integer" 9223372036854775808
                           "nil" '()
                           "symbol" 'ready
                           "result" (ok-value (list 1 #t "value"))))))

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

(define schema-program-source
  (string-append
   "(program (name schema-demo) (version 1) (capabilities) "
   "(schema create-input "
   "  (object "
   "    (required \"id\" (string 1 8)) "
   "    (required \"title\" (string 1 12)) "
   "    (optional \"completed\" boolean #f))) "
   "(schema integer-list (list (integer 0 9) 0 100)) "
   "(def check (fn (value) (validate create-input value))) "
   "(def check-list (fn (value) (validate integer-list value))) "
   "(def fail (fn (details) "
   "  (api-error 422 \"VALIDATION_FAILED\" \"input was rejected\" details))) "
   "(export check check-list fail))"))

(test "schemas are compiler-owned program metadata"
      (lambda ()
        (define program (load-program-source schema-program-source))
        (check-equal (length (ail-program-schemas program)) 2)
        (define inspected (program->jsexpr program))
        (check-equal
         (hash-ref (car (hash-ref inspected 'schemas)) 'name)
         "create-input")
        (check-equal
         (hash-ref
          (hash-ref (car (hash-ref inspected 'schemas)) 'schema)
          'additionalProperties)
         #f)))

(test "schema validation normalizes defaults and returns bounded issues"
      (lambda ()
        (define program (load-program-source schema-program-source))
        (define accepted
          (execute-export program
                          'check
                          (list (hash "id" "a" "title" "ship"))))
        (check-true (ok-value? accepted))
        (check-equal
         (ok-value-value accepted)
         (hash "id" "a" "title" "ship" "completed" #f))
        (define rejected
          (execute-export
           program
           'check
           (list (hash "id" "" "owner" "ambient"))))
        (check-true (err-value? rejected))
        (define issues (err-value-value rejected))
        (check-equal (map (lambda (issue) (hash-ref issue "code")) issues)
                     '("SCHEMA_MIN_LENGTH"
                       "SCHEMA_REQUIRED"
                       "SCHEMA_ADDITIONAL_PROPERTY"))
        (check-equal (map (lambda (issue) (hash-ref issue "path")) issues)
                     '("/id" "/title" "/owner"))
        (define capped
          (execute-export program
                          'check-list
                          (list (make-list 40 "not-an-integer"))))
        (check-true (err-value? capped))
        (define capped-issues (err-value-value capped))
        (check-equal (length capped-issues) 32)
        (check-equal (hash-ref (last capped-issues) "code")
                     "SCHEMA_ISSUES_TRUNCATED")))

(test "invalid schema defaults and reserved names fail during parsing"
      (lambda ()
        (check-ail-code
         "PROGRAM_SCHEMA_INVALID_DEFAULT"
         (lambda ()
           (load-program-source
            (string-append
             "(program (name bad-default) (version 1) (capabilities) "
             "(schema input (object (optional \"enabled\" boolean \"yes\"))) "
             "(def run (fn () #t)) (export run))"))))
        (check-ail-code
         "PROGRAM_SCHEMA_RESERVED_NAME"
         (lambda ()
           (load-program-source
            (string-append
             "(program (name bad-name) (version 1) (capabilities) "
             "(schema map string) "
             "(def run (fn () #t)) (export run))"))))))

(test "schema work consumes interpreter fuel"
      (lambda ()
        (define program (load-program-source schema-program-source))
        (check-ail-code
         "RUNTIME_FUEL_EXHAUSTED"
         (lambda ()
           (execute-export program
                           'check-list
                           (list (make-list 50 1))
                           #:fuel 20)))))

(test "API error constructor produces one stable public envelope"
      (lambda ()
        (define program (load-program-source schema-program-source))
        (define response
          (execute-export program 'fail (list (list (hash "path" "/id")))))
        (check-equal (hash-ref response "status") 422)
        (check-equal (hash-ref response "headers") (hash))
        (define error-body (hash-ref (hash-ref response "body") "error"))
        (check-equal (hash-ref error-body "code") "VALIDATION_FAILED")
        (check-equal (hash-ref error-body "message") "input was rejected")
        (check-equal (hash-ref error-body "details")
                     (list (hash "path" "/id")))))

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

(define text-library-source
  (string-append
   "(program (name text-demo) (version 1) (capabilities) "
   "(libraries (text 1)) "
   "(def analyze (fn (value) "
   "  (list (text/length value) "
   "        (text/starts-with? value \"AI\") "
   "        (text/ends-with? value \"语言\") "
   "        (text/contains? value \"I语\") "
   "        (text/replace value \"AI\" \"人工智能\")))) "
   "(def measure (fn (value) (text/length value))) "
   "(export analyze measure))"))

(define (test-text-implementations length-implementation)
  (hasheq
   'text/length length-implementation
   'text/starts-with? (lambda (_arguments) #f)
   'text/ends-with? (lambda (_arguments) #f)
   'text/contains? (lambda (_arguments) #f)
   'text/replace (lambda (arguments) (car arguments))))

(test "versioned libraries are compiler-owned inspectable metadata"
      (lambda ()
        (define program (load-program-source text-library-source))
        (check-equal (length (ail-program-libraries program)) 1)
        (define inspected (program->jsexpr program))
        (check-equal (hash-ref inspected 'libraries)
                     (list (hasheq 'name "text" 'version 1)))
        (check-equal
         (execute-export program 'analyze (list "AI语言"))
         (list 4 #t #t #t "人工智能语言"))))

(test "text library example passes its portable JSON suite"
      (lambda ()
        (define program
          (load-program-file (build-path library-example-root "text.ail")))
        (define suite
          (load-test-suite (build-path library-example-root "tests.json")))
        (define report (run-test-suite program suite))
        (check-true (hash-ref report 'passed))
        (check-equal (hash-ref report 'total) 2)))

(test "library declarations reject unknown duplicate and conflicting contracts"
      (lambda ()
        (check-ail-code
         "PROGRAM_UNKNOWN_LIBRARY"
         (lambda ()
           (load-program-source
            (string-append
             "(program (name unknown-lib) (version 1) (capabilities) "
             "(libraries (text 2)) (def run (fn () #t)) (export run))"))))
        (check-ail-code
         "PROGRAM_DUPLICATE_LIBRARY"
         (lambda ()
           (load-program-source
            (string-append
             "(program (name duplicate-lib) (version 1) (capabilities) "
             "(libraries (text 1) (text 1)) "
             "(def run (fn () #t)) (export run))"))))
        (check-ail-code
         "PROGRAM_LIBRARY_NAMESPACE_CONFLICT"
         (lambda ()
           (load-program-source
            (string-append
             "(program (name shadow-lib) (version 1) (capabilities) "
             "(libraries (text 1)) "
             "(def text/length (fn (value) 0)) "
             "(export text/length))"))))))

(test "undeclared and unavailable library functions have no ambient authority"
      (lambda ()
        (define undeclared
          (load-program-source
           (string-append
            "(program (name no-library) (version 1) (capabilities) "
            "(def run (fn (value) (text/length value))) (export run))")))
        (check-ail-code
         "RUNTIME_UNBOUND_NAME"
         (lambda () (execute-export undeclared 'run (list "hidden"))))
        (define declared (load-program-source text-library-source))
        (check-ail-code
         "RUNTIME_LIBRARY_UNAVAILABLE"
         (lambda ()
           (execute-export declared
                           'measure
                           (list "missing")
                           #:library-backends (hash))))))

(test "host can replace a library backend without changing guest source"
      (lambda ()
        (define program (load-program-source text-library-source))
        (define alternate
          (library-backend
           'text
           1
           "rust-conformance-test"
           (test-text-implementations (lambda (_arguments) 9001))))
        (check-equal
         (execute-export
          program
          'analyze
          (list "AI语言")
          #:library-backends (hash (cons 'text 1) alternate))
         (list 9001 #f #f #f "AI语言"))))

(test "library contract owns types fuel and the exact function set"
      (lambda ()
        (define program (load-program-source text-library-source))
        (check-ail-code
         "RUNTIME_TYPE"
         (lambda () (execute-export program 'measure (list 42))))
        (check-ail-code
         "RUNTIME_FUEL_EXHAUSTED"
         (lambda ()
           (execute-export program
                           'measure
                           (list (make-string 1024 #\a))
                           #:fuel 10)))
        (define missing-function
          (library-backend
           'text
           1
           "incomplete"
           (hash-remove (test-text-implementations
                         (lambda (_arguments) 1))
                        'text/contains?)))
        (check-ail-code
         "RUNTIME_INVALID_LIBRARY_BACKEND"
         (lambda ()
           (execute-export
            program
            'measure
            (list "x")
            #:library-backends
            (hash (cons 'text 1) missing-function))))
        (define extra-function
          (library-backend
           'text
           1
           "too-wide"
           (hash-set (test-text-implementations (lambda (_arguments) 1))
                     'text/secret
                     (lambda (_arguments) "ambient"))))
        (check-ail-code
         "RUNTIME_INVALID_LIBRARY_BACKEND"
         (lambda ()
           (execute-export
            program
            'measure
            (list "x")
            #:library-backends
            (hash (cons 'text 1) extra-function))))
        (define wrong-version
          (library-backend
           'text
           2
           "wrong-version"
           (test-text-implementations (lambda (_arguments) 1))))
        (check-ail-code
         "RUNTIME_INVALID_LIBRARY_BACKEND"
         (lambda ()
           (execute-export
            program
            'measure
            (list "x")
            #:library-backends
            (hash (cons 'text 1) wrong-version))))
        (define malformed-version
          (library-backend
           'text
           "1"
           "malformed-version"
           (test-text-implementations (lambda (_arguments) 1))))
        (check-ail-code
         "RUNTIME_INVALID_LIBRARY_BACKEND"
         (lambda ()
           (execute-export
            program
            'measure
            (list "x")
            #:library-backends
            (hash (cons 'text 1) malformed-version))))))

(test "library failures are redacted and results stay inside the value boundary"
      (lambda ()
        (define program (load-program-source text-library-source))
        (define failing
          (library-backend
           'text
           1
           "failing-provider"
           (test-text-implementations
            (lambda (_arguments) (error 'backend "secret-token-value")))))
        (define observed #f)
        (with-handlers ([exn:fail:ail? (lambda (error) (set! observed error))])
          (execute-export
           program
           'measure
           (list "x")
           #:library-backends (hash (cons 'text 1) failing)))
        (check-true observed)
        (check-equal (exn:fail:ail-code observed) "RUNTIME_LIBRARY_FAILURE")
        (check-false (regexp-match? #px"secret-token-value"
                                    (format "~s" (ail-error->jsexpr observed))))
        (define invalid-result
          (library-backend
           'text
           1
           "invalid-result"
           (test-text-implementations (lambda (_arguments) void))))
        (check-ail-code
         "RUNTIME_LIBRARY_INVALID_RESULT"
         (lambda ()
           (execute-export
            program
            'measure
            (list "x")
            #:library-backends
            (hash (cons 'text 1) invalid-result))))
        (define oversized-result
          (library-backend
           'text
           1
           "oversized-result"
           (hash-set
            (test-text-implementations (lambda (_arguments) 1))
            'text/replace
            (lambda (_arguments) (make-string 1048577 #\x)))))
        (check-ail-code
         "RUNTIME_LIBRARY_INVALID_RESULT"
         (lambda ()
           (execute-export
            program
            'analyze
            (list "AI语言")
            #:fuel 50000
            #:library-backends
            (hash (cons 'text 1) oversized-result))))))

(test "service execution uses the host-selected library backend"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name library-service) (version 1) (capabilities) "
            "(libraries (text 1)) "
            "(route GET \"/measure\" handler) "
            "(def handler (fn (request) "
            "  (api-response 200 (text/length (get request \"path\"))))) "
            "(export handler))")))
        (define alternate
          (library-backend
           'text
           1
           "service-test"
           (test-text-implementations (lambda (_arguments) 77))))
        (define result
          (handle-service-request
           program
           (service-request "GET" "/measure" (hash) (hash) '())
           #:library-backends (hash (cons 'text 1) alternate)))
        (check-false (dispatch-result-diagnostic result))
        (check-equal
         (service-response-body (dispatch-result-response result))
         77)))

(define task-service-source
  (string-append
   "(program (name task-service) (version 1) (capabilities kv clock) "
   "(route POST \"/tasks\" create-task) "
   "(route GET \"/tasks/:id\" get-task) "
   "(route POST \"/fail\" failing-write) "
   "(def response (fn (status body) "
   "  (map \"status\" status \"headers\" (map) \"body\" body))) "
   "(def create-task (fn (request) "
   "  (let ((body (get request \"body\")) "
   "        (id (get body \"id\")) "
   "        (task (assoc body \"createdAt\" (now-ms)))) "
   "    (do (kv-put (string-append \"task/\" id) task) "
   "        (response 201 task))))) "
   "(def get-task (fn (request) "
   "  (let ((id (get (get request \"params\") \"id\")) "
   "        (task (kv-get (string-append \"task/\" id) #f))) "
   "    (if task (response 200 task) "
   "        (response 404 (map \"error\" \"not-found\")))))) "
   "(def failing-write (fn (request) "
   "  (do (kv-put \"task/should-rollback\" (map \"id\" \"bad\")) "
   "      (map \"status\" 200)))) "
   "(export create-task get-task failing-write))"))

(test "service dispatch matches path parameters and commits valid KV writes"
      (lambda ()
        (define program (load-program-source task-service-source))
        (define store (make-memory-kv-store))
        (define create-result
          (handle-service-request
           program
           (service-request "POST"
                            "/tasks"
                            (hash)
                            (hash)
                            (hash "id" "42" "title" "ship"))
           #:store store
           #:clock (lambda () 123456)))
        (check-false (dispatch-result-diagnostic create-result))
        (check-equal
         (service-response-status (dispatch-result-response create-result))
         201)
        (check-equal
         (hash-ref (service-response-body
                    (dispatch-result-response create-result))
                   'createdAt)
         123456)
        (define get-result
          (handle-service-request
           program
           (service-request "GET" "/tasks/42" (hash) (hash) '())
           #:store store
           #:clock (lambda () 999999)))
        (check-equal
         (service-response-status (dispatch-result-response get-result))
         200)
        (check-equal
         (hash-ref (service-response-body
                    (dispatch-result-response get-result))
                   'title)
         "ship")))

(test "invalid handler responses roll back transactional KV writes"
      (lambda ()
        (define program (load-program-source task-service-source))
        (define store (make-memory-kv-store))
        (define result
          (handle-service-request
           program
           (service-request "POST" "/fail" (hash) (hash) (hash))
           #:store store))
        (check-equal
         (service-response-status (dispatch-result-response result))
         500)
        (define public-body
          (service-response-body (dispatch-result-response result)))
        (check-api-error-envelope public-body "INTERNAL_ERROR")
        (check-equal
         (hash-ref (hash-ref (hash-ref public-body 'error) 'details) 'requestId)
         (hash-ref (dispatch-result-diagnostic result) 'requestId))
        (check-true (dispatch-result-diagnostic result))
        (check-false
         (hash-has-key? (kv-store-snapshot store) "task/should-rollback"))))

(test "guest response headers cannot inject additional HTTP fields"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name unsafe-header) (version 1) (capabilities) "
            "(route GET \"/\" handler) "
            "(def handler (fn (request) "
            "  (map \"status\" 200 "
            "       \"headers\" (map \"x-note\" \"safe\\r\\nInjected: true\") "
            "       \"body\" (map)))) "
            "(export handler))")))
        (define result
          (handle-service-request
           program
           (service-request "GET" "/" (hash) (hash) '())))
        (check-equal
         (service-response-status (dispatch-result-response result))
         500)
        (check-equal
         (hash-ref (hash-ref (dispatch-result-diagnostic result) 'error) 'code)
         "SERVICE_INVALID_RESPONSE_HEADERS")))

(test "file KV adapter survives reopen with JSON business values"
      (lambda ()
        (define directory (make-temporary-file "ai-evolve-kv-~a" 'directory))
        (define path (build-path directory "tasks.json"))
        (dynamic-wind
          void
          (lambda ()
            (define program (load-program-source task-service-source))
            (define first-store (open-file-kv-store path))
            (define result
              (handle-service-request
               program
               (service-request "POST"
                                "/tasks"
                                (hash)
                                (hash)
                                (hash "id" "7" "title" "persist"))
               #:store first-store
               #:clock (lambda () 7)))
            (check-equal
             (service-response-status (dispatch-result-response result))
             201)
            (define reopened (open-file-kv-store path))
            (check-equal
             (hash-ref (hash-ref (kv-store-snapshot reopened) "task/7") "title")
             "persist"))
          (lambda () (delete-directory/files directory)))))

(define (send-http-request port method path [body #f] [content-type "application/json"])
  (define-values (input output) (tcp-connect "127.0.0.1" port))
  (define body-text
    (cond
      [(not body) ""]
      [(string? body) body]
      [else (jsexpr->string body)]))
  (display (format "~a ~a HTTP/1.1\r\n" method path) output)
  (display "Host: 127.0.0.1\r\n" output)
  (when body
    (display (format "Content-Type: ~a\r\n" content-type) output)
    (display (format "Content-Length: ~a\r\n"
                     (bytes-length (string->bytes/utf-8 body-text)))
             output))
  (display "Connection: close\r\n\r\n" output)
  (when body (display body-text output))
  (flush-output output)
  (define response-text (bytes->string/utf-8 (port->bytes input)))
  (close-input-port input)
  (close-output-port output)
  (define parts (regexp-split #px"\r\n\r\n" response-text))
  (unless (>= (length parts) 2)
    (error 'send-http-request "server returned a malformed HTTP response"))
  (define status-match
    (regexp-match #px"^HTTP/1[.]1 ([0-9]{3})" (car parts)))
  (unless status-match
    (error 'send-http-request "server returned a malformed status line"))
  (values (string->number (cadr status-match))
          (string->jsexpr (string-join (cdr parts) "\r\n\r\n"))))

(define (send-http-text port path)
  (define-values (input output) (tcp-connect "127.0.0.1" port))
  (display (format "GET ~a HTTP/1.1\r\n" path) output)
  (display "Host: 127.0.0.1\r\nConnection: close\r\n\r\n" output)
  (flush-output output)
  (define response-text (bytes->string/utf-8 (port->bytes input)))
  (close-input-port input)
  (close-output-port output)
  (define parts (regexp-split #px"\r\n\r\n" response-text))
  (define status-match
    (and (pair? parts)
         (regexp-match #px"^HTTP/1[.]1 ([0-9]{3})" (car parts))))
  (unless (and status-match (>= (length parts) 2))
    (error 'send-http-text "server returned a malformed HTTP response"))
  (values (string->number (cadr status-match))
          (string-join (cdr parts) "\r\n\r\n")))

(test "HTTP host preserves the selected library backend"
      (lambda ()
        (define program
          (load-program-source
           (string-append
            "(program (name http-library) (version 1) (capabilities) "
            "(libraries (text 1)) "
            "(route GET \"/measure\" handler) "
            "(def handler (fn (request) "
            "  (api-response 200 (text/length (get request \"path\"))))) "
            "(export handler))")))
        (define backend
          (library-backend
           'text
           1
           "http-test"
           (test-text-implementations (lambda (_arguments) 88))))
        (define server
          (start-http-server
           (lambda () program)
           #:port 0
           #:library-backends (hash (cons 'text 1) backend)))
        (dynamic-wind
          void
          (lambda ()
            (define-values (status body)
              (send-http-request
               (running-http-server-port server)
               "GET"
               "/measure"))
            (check-equal status 200)
            (check-equal body 88))
          (lambda () (stop-http-server! server)))))

(test "real TCP server accepts JSON requests and returns persisted service data"
      (lambda ()
        (define program (load-program-source task-service-source))
        (define store (make-memory-kv-store))
        (define observations '())
        (define server
          (start-http-server
           (lambda () program)
           #:store store
           #:port 0
           #:observer
           (lambda (observation)
             (set! observations (cons observation observations)))))
        (dynamic-wind
          void
          (lambda ()
            (define-values (create-status create-body)
              (send-http-request
               (running-http-server-port server)
               "POST"
               "/tasks"
               (hasheq 'id "tcp-1" 'title "over tcp")))
            (check-equal create-status 201)
            (check-equal (hash-ref create-body 'title) "over tcp")
            (define-values (get-status get-body)
              (send-http-request
               (running-http-server-port server)
               "GET"
               "/tasks/tcp-1"))
            (check-equal get-status 200)
            (check-equal (hash-ref get-body 'id) "tcp-1")
            (check-equal (length observations) 2)
            (check-equal (hash-ref (car observations) 'status) 200))
          (lambda () (stop-http-server! server)))))

(test "HTTP boundary rejects malformed JSON before guest execution"
      (lambda ()
        (define program (load-program-source task-service-source))
        (define store (make-memory-kv-store))
        (define server
          (start-http-server (lambda () program) #:store store #:port 0))
        (dynamic-wind
          void
          (lambda ()
            (define-values (status body)
              (send-http-request
               (running-http-server-port server)
               "POST"
               "/tasks"
               "{broken"))
            (check-equal status 400)
            (check-equal
             (hash-ref (hash-ref body 'error) 'code)
             "HTTP_INVALID_JSON")
            (check-api-error-envelope body "HTTP_INVALID_JSON")
            (check-equal (hash-count (kv-store-snapshot store)) 0))
          (lambda () (stop-http-server! server)))))

(test "HTTP host serves the responsive task UI from an explicit asset allowlist"
      (lambda ()
        (define program (load-program-file task-example-path))
        (define server
          (start-http-server
           (lambda () program)
           #:store (make-memory-kv-store)
           #:port 0
           #:static-root (build-path project-root "web" "tasks")))
        (dynamic-wind
          void
          (lambda ()
            (define-values (page-status page)
              (send-http-text (running-http-server-port server) "/"))
            (check-equal page-status 200)
            (check-true (string-contains? page "Task Ledger"))
            (check-true (string-contains? page "viewport"))
            (define-values (script-status script)
              (send-http-text (running-http-server-port server) "/app.js"))
            (check-equal script-status 200)
            (check-true (string-contains? script "async function api")))
          (lambda () (stop-http-server! server)))))

(test "task backend completes CRUD over HTTP and survives server restart"
      (lambda ()
        (define directory
          (make-temporary-file "ai-evolve-http-crud-~a" 'directory))
        (define store-path (build-path directory "store.json"))
        (define program (load-program-file task-example-path))
        (define (start)
          (start-http-server
           (lambda () program)
           #:store (open-file-kv-store store-path)
           #:port 0))
        (dynamic-wind
          void
          (lambda ()
            (define first-server (start))
            (dynamic-wind
              void
              (lambda ()
                (define port (running-http-server-port first-server))
                (define-values (invalid-status invalid-body)
                  (send-http-request port "POST" "/tasks" "\"not-an-object\""))
                (check-equal invalid-status 400)
                (check-api-error-envelope invalid-body "VALIDATION_FAILED")
                (define invalid-details
                  (hash-ref (hash-ref invalid-body 'error) 'details))
                (check-equal (hash-ref (car invalid-details) 'path) "")
                (check-equal (hash-ref (car invalid-details) 'code)
                             "SCHEMA_TYPE")
                (define-values (created-status created)
                  (send-http-request
                   port
                   "POST"
                   "/tasks"
                   (hasheq 'id "business-1"
                           'title "first title"
                           'completed #f)))
                (check-equal created-status 201)
                (check-equal (hash-ref created 'completed) #f)
                (define-values (duplicate-status _duplicate)
                  (send-http-request
                   port
                   "POST"
                   "/tasks"
                   (hasheq 'id "business-1" 'title "duplicate")))
                (check-equal duplicate-status 409)
                (define-values (list-status tasks)
                  (send-http-request port "GET" "/tasks"))
                (check-equal list-status 200)
                (check-equal (length tasks) 1)
                (define-values (update-status updated)
                  (send-http-request
                   port
                   "PUT"
                   "/tasks/business-1"
                   (hasheq 'title "updated title" 'completed #t)))
                (check-equal update-status 200)
                (check-equal (hash-ref updated 'title) "updated title")
                (check-true (hash-ref updated 'completed)))
              (lambda () (stop-http-server! first-server)))
            (define second-server (start))
            (dynamic-wind
              void
              (lambda ()
                (define port (running-http-server-port second-server))
                (define-values (get-status persisted)
                  (send-http-request port "GET" "/tasks/business-1"))
                (check-equal get-status 200)
                (check-equal (hash-ref persisted 'title) "updated title")
                (define-values (method-status method-body)
                  (send-http-request port "PATCH" "/tasks/business-1" (hasheq)))
                (check-equal method-status 405)
                (check-api-error-envelope method-body "METHOD_NOT_ALLOWED")
                (define-values (delete-status deleted)
                  (send-http-request port "DELETE" "/tasks/business-1"))
                (check-equal delete-status 200)
                (check-equal (hash-ref deleted 'id) "business-1")
                (define-values (missing-status missing)
                  (send-http-request port "GET" "/tasks/business-1"))
                (check-equal missing-status 404)
                (check-api-error-envelope missing "TASK_NOT_FOUND"))
              (lambda () (stop-http-server! second-server))))
          (lambda () (delete-directory/files directory)))))

(test "stateful service scenario suite gates web backend candidates"
      (lambda ()
        (define source (file->string task-example-path))
        (define suite
          (load-service-test-suite
           (build-path project-root "examples" "tasks" "scenarios.json")))
        (define passing-report
          (run-service-test-suite (load-program-source source) suite))
        (check-true (hash-ref passing-report 'passed))
        (check-equal (hash-ref passing-report 'total) 11)
        (define broken-source
          (string-replace source
                          "(api-response 201 task)"
                          "(api-response 200 task)"))
        (define failing-report
          (run-service-test-suite (load-program-source broken-source) suite))
        (check-false (hash-ref failing-report 'passed))
        (check-true (positive? (hash-ref failing-report 'failedCount)))))

(test "service deployment promotes only a scenario-tested active version"
      (lambda ()
        (define directory
          (make-temporary-file "ai-evolve-service-deploy-~a" 'directory))
        (dynamic-wind
          void
          (lambda ()
            (define source (file->string task-example-path))
            (define suite
              (load-service-test-suite
               (build-path project-root "examples" "tasks" "scenarios.json")))
            (define deployed (deploy-service! source suite directory))
            (check-true (hash-ref deployed 'ok))
            (check-true (hash-ref deployed 'promoted))
            (check-equal (active-hash directory) (source-hash source))
            (define loader (make-active-program-loader directory))
            (check-equal (ail-program-name (loader)) 'task-service)
            (define upgraded-source
              (string-replace source "(version 2)" "(version 3)"))
            (define upgraded (deploy-service! upgraded-source suite directory))
            (check-true (hash-ref upgraded 'promoted))
            (check-equal (ail-program-version (loader)) 3)
            (define broken-source
              (string-replace upgraded-source
                              "(api-response 201 task)"
                              "(api-response 200 task)"))
            (define rejected (deploy-service! broken-source suite directory))
            (check-false (hash-ref rejected 'ok))
            (check-false (hash-ref rejected 'promoted))
            (check-equal (active-hash directory) (source-hash upgraded-source)))
          (lambda () (delete-directory/files directory)))))

(test "service evolution uses stateful scenarios before promotion"
      (lambda ()
        (define directory
          (make-temporary-file "ai-evolve-service-evolve-~a" 'directory))
        (dynamic-wind
          void
          (lambda ()
            (define source (file->string task-example-path))
            (define broken-source
              (string-replace source
                              "(api-response 201 task)"
                              "(api-response 200 task)"))
            (define broken-hash
              (register-candidate! directory
                                   broken-source
                                   #:provider "test-bootstrap"
                                   #:report (hasheq 'passed #t)))
            (promote! directory broken-hash)
            (define suite
              (load-service-test-suite
               (build-path project-root "examples" "tasks" "scenarios.json")))
            (define result
              (evolve-active-service-once
               directory
               suite
               (make-file-provider task-example-path)
               #:promote? #t))
            (check-true (hash-ref result 'ok))
            (check-true (hash-ref result 'promoted))
            (check-equal (active-hash directory) (source-hash source))
            (check-equal
             (hash-ref (hash-ref (hash-ref result 'candidate) 'report) 'total)
             11))
          (lambda () (delete-directory/files directory)))))

(test "concurrent HTTP writes preserve every committed task"
      (lambda ()
        (define program (load-program-file task-example-path))
        (define store (make-memory-kv-store))
        (define server
          (start-http-server
           (lambda () program)
           #:store store
           #:port 0
           #:max-workers 16))
        (dynamic-wind
          void
          (lambda ()
            (define result-channel (make-channel))
            (for ([index (in-range 12)])
              (thread
               (lambda ()
                 (with-handlers ([exn:fail?
                                  (lambda (error)
                                    (channel-put result-channel error))])
                   (define-values (status body)
                     (send-http-request
                      (running-http-server-port server)
                      "POST"
                      "/tasks"
                      (hasheq 'id (format "concurrent-~a" index)
                              'title (format "task ~a" index))))
                   (channel-put result-channel (cons status body))))))
            (for ([_index (in-range 12)])
              (define result (channel-get result-channel))
              (when (exn:fail? result) (raise result))
              (check-equal (car result) 201))
            (define-values (status tasks)
              (send-http-request
               (running-http-server-port server)
               "GET"
               "/tasks"))
            (check-equal status 200)
            (check-equal (length tasks) 12)
            (check-equal (hash-count (kv-store-snapshot store)) 12))
          (lambda () (stop-http-server! server)))))

(test "HTTP request remains pinned to the program selected at request start"
      (lambda ()
        (define (version-program version text)
          (load-program-source
           (format
            (string-append
             "(program (name pinned) (version ~a) (capabilities) "
             "(route GET \"/version\" version-handler) "
             "(def version-handler (fn (request) "
             "  (map \"status\" 200 \"headers\" (map) \"body\" \"~a\"))) "
             "(export version-handler))")
            version text)))
        (define first-program (version-program 1 "v1"))
        (define second-program (version-program 2 "v2"))
        (define active-program (box first-program))
        (define selected-channel (make-channel))
        (define continue-semaphore (make-semaphore 0))
        (define first-load? #t)
        (define (loader)
          (define selected (unbox active-program))
          (when first-load?
            (set! first-load? #f)
            (channel-put selected-channel #t)
            (semaphore-wait continue-semaphore))
          selected)
        (define server (start-http-server loader #:port 0))
        (dynamic-wind
          void
          (lambda ()
            (define response-channel (make-channel))
            (thread
             (lambda ()
               (define-values (status body)
                 (send-http-request
                  (running-http-server-port server)
                  "GET"
                  "/version"))
               (channel-put response-channel (cons status body))))
            (channel-get selected-channel)
            (set-box! active-program second-program)
            (semaphore-post continue-semaphore)
            (define first-response (channel-get response-channel))
            (check-equal (car first-response) 200)
            (check-equal (cdr first-response) "v1")
            (define-values (second-status second-body)
              (send-http-request
               (running-http-server-port server)
               "GET"
               "/version"))
            (check-equal second-status 200)
            (check-equal second-body "v2"))
          (lambda () (stop-http-server! server)))))

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
        (check-true
         (string-contains? (hash-ref body 'instructions)
                           "(schema NAME SCHEMA)"))
        (check-true
         (string-contains? (hash-ref body 'instructions) "api-error"))
        (check-true
         (string-contains? (hash-ref body 'instructions)
                           "(libraries (LOWERCASE-NAME VERSION) ...)"))
        (check-true
         (string-contains? (hash-ref body 'instructions)
                           "explain why"))
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
        (check-true
         (string-contains?
          (hash-ref (car (hash-ref body 'messages)) 'content)
          "(schema NAME SCHEMA)"))
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
