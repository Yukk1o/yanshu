#lang racket/base

(require json
         racket/file
         "error.rkt"
         "kv-store.rkt"
         "runtime.rkt"
         "service.rkt")

(provide (struct-out service-test-suite)
         (struct-out service-test-case)
         load-service-test-suite
         run-service-test-suite)

(struct service-test-suite (clock-ms cases) #:transparent)
(struct service-test-case
  (name method path query headers body expected-status expected-body expected-contains)
  #:transparent)

(define (load-service-test-suite path)
  (define document (call-with-input-file path read-json))
  (unless (hash? document)
    (raise-ail "SERVICE_TEST_INVALID_DOCUMENT"
               "service test suite must be a JSON object"))
  (define clock-ms (hash-ref document 'clockMs 1700000000000))
  (unless (exact-integer? clock-ms)
    (raise-ail "SERVICE_TEST_INVALID_CLOCK"
               "service test clockMs must be an integer"))
  (define cases (required-key document 'cases))
  (unless (list? cases)
    (raise-ail "SERVICE_TEST_INVALID_CASES"
               "service test cases must be a JSON array"))
  (service-test-suite
   clock-ms
   (for/list ([case-document (in-list cases)]
              [index (in-naturals)])
     (parse-service-test-case case-document index))))

(define (parse-service-test-case document index)
  (unless (hash? document)
    (raise-ail "SERVICE_TEST_INVALID_CASE"
               "service test case must be a JSON object"
               (hasheq 'index index)))
  (define name (hash-ref document 'name (format "case-~a" index)))
  (define method (required-key document 'method))
  (define path (required-key document 'path))
  (define expected-status (required-key document 'expectStatus))
  (unless (and (string? name)
               (string? method)
               (string? path)
               (exact-integer? expected-status)
               (<= 100 expected-status 599))
    (raise-ail "SERVICE_TEST_INVALID_CASE"
               "service test case has invalid name, request, or status"
               (hasheq 'index index)))
  (define raw-query (hash-ref document 'query (hasheq)))
  (define raw-headers (hash-ref document 'headers (hasheq)))
  (unless (and (hash? raw-query) (hash? raw-headers))
    (raise-ail "SERVICE_TEST_INVALID_CASE"
               "service test query and headers must be JSON objects"
               (hasheq 'name name)))
  (define has-exact? (hash-has-key? document 'expectBody))
  (define has-contains? (hash-has-key? document 'expectBodyContains))
  (when (and has-exact? has-contains?)
    (raise-ail "SERVICE_TEST_AMBIGUOUS_EXPECTATION"
               "service test cannot define both expectBody and expectBodyContains"
               (hasheq 'name name)))
  (service-test-case
   name
   method
   path
   (jsexpr->value raw-query)
   (jsexpr->value raw-headers)
   (jsexpr->value (hash-ref document 'body (json-null)))
   expected-status
   (and has-exact? (hash-ref document 'expectBody))
   (and has-contains? (hash-ref document 'expectBodyContains))))

(define (run-service-test-suite program suite)
  (unless (service-test-suite? suite)
    (raise-argument-error 'run-service-test-suite "service-test-suite?" suite))
  (define store (make-memory-kv-store))
  (define failures '())
  (define passed-count 0)
  (for ([test-case (in-list (service-test-suite-cases suite))])
    (define failure
      (run-service-test-case program
                             store
                             (service-test-suite-clock-ms suite)
                             test-case))
    (if failure
        (set! failures (cons failure failures))
        (set! passed-count (add1 passed-count))))
  (define total (length (service-test-suite-cases suite)))
  (hasheq 'passed (null? failures)
          'total total
          'passedCount passed-count
          'failedCount (length failures)
          'failures (reverse failures)))

(define (run-service-test-case program store clock-ms test-case)
  (with-handlers
      ([exn:fail:ail?
        (lambda (error)
          (hasheq 'name (service-test-case-name test-case)
                  'reason "suite-error"
                  'actual (hash-ref (ail-error->jsexpr error) 'error)))])
    (define result
      (handle-service-request
       program
       (service-request (service-test-case-method test-case)
                        (service-test-case-path test-case)
                        (service-test-case-query test-case)
                        (service-test-case-headers test-case)
                        (service-test-case-body test-case))
       #:store store
       #:clock (lambda () clock-ms)))
    (define response (dispatch-result-response result))
    (define actual-status (service-response-status response))
    (define actual-body (service-response-body response))
    (cond
      [(not (= actual-status (service-test-case-expected-status test-case)))
       (hasheq 'name (service-test-case-name test-case)
               'reason "status-mismatch"
               'expectedStatus (service-test-case-expected-status test-case)
               'actualStatus actual-status
               'actualBody actual-body)]
      [(and (service-test-case-expected-body test-case)
            (not (equal? actual-body
                         (service-test-case-expected-body test-case))))
       (hasheq 'name (service-test-case-name test-case)
               'reason "body-mismatch"
               'expected (service-test-case-expected-body test-case)
               'actual actual-body)]
      [(and (service-test-case-expected-contains test-case)
            (not (jsexpr-contains?
                  actual-body
                  (service-test-case-expected-contains test-case))))
       (hasheq 'name (service-test-case-name test-case)
               'reason "body-does-not-contain"
               'expectedContains
               (service-test-case-expected-contains test-case)
               'actual actual-body)]
      [else #f])))

(define (jsexpr-contains? actual expected)
  (cond
    [(hash? expected)
     (and (hash? actual)
          (for/and ([(key expected-value) (in-hash expected)])
            (and (hash-has-key? actual key)
                 (jsexpr-contains? (hash-ref actual key) expected-value))))]
    [(list? expected)
     (and (list? actual)
          (= (length actual) (length expected))
          (for/and ([actual-value (in-list actual)]
                    [expected-value (in-list expected)])
            (jsexpr-contains? actual-value expected-value)))]
    [else (equal? actual expected)]))

(define (required-key document key)
  (hash-ref
   document
   key
   (lambda ()
     (raise-ail "SERVICE_TEST_MISSING_FIELD"
                "service test document is missing a required field"
                (hasheq 'field (symbol->string key))))))
