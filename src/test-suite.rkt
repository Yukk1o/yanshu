#lang racket/base

(require json
         racket/file
         "error.rkt"
         "runtime.rkt")

(provide (struct-out ail-test-suite)
         (struct-out ail-test-case)
         load-test-suite
         run-test-suite)

(struct ail-test-suite (entry cases) #:transparent)
(struct ail-test-case (name arguments expected expected-error) #:transparent)

(define (load-test-suite path)
  (define document
    (call-with-input-file path read-json))
  (unless (hash? document)
    (raise-ail "TEST_INVALID_DOCUMENT"
               "test suite must be a JSON object"))
  (define entry-value (required-key document 'entry))
  (unless (string? entry-value)
    (raise-ail "TEST_INVALID_ENTRY"
               "test suite entry must be a string"))
  (define case-values (required-key document 'cases))
  (unless (list? case-values)
    (raise-ail "TEST_INVALID_CASES"
               "test suite cases must be a JSON array"))
  (ail-test-suite
   (string->symbol entry-value)
   (for/list ([case-value (in-list case-values)]
              [index (in-naturals)])
     (parse-test-case case-value index))))

(define (parse-test-case document index)
  (unless (hash? document)
    (raise-ail "TEST_INVALID_CASE"
               "test case must be a JSON object"
               (hasheq 'index index)))
  (define name (hash-ref document 'name (lambda () (format "case-~a" index))))
  (unless (string? name)
    (raise-ail "TEST_INVALID_CASE_NAME"
               "test case name must be a string"
               (hasheq 'index index)))
  (define arguments (required-key document 'args))
  (unless (list? arguments)
    (raise-ail "TEST_INVALID_ARGUMENTS"
               "test case args must be a JSON array"
               (hasheq 'name name)))
  (define has-expected? (hash-has-key? document 'expect))
  (define has-error? (hash-has-key? document 'expectError))
  (unless (or has-expected? has-error?)
    (raise-ail "TEST_MISSING_EXPECTATION"
               "test case must define expect or expectError"
               (hasheq 'name name)))
  (when (and has-expected? has-error?)
    (raise-ail "TEST_AMBIGUOUS_EXPECTATION"
               "test case cannot define both expect and expectError"
               (hasheq 'name name)))
  (define expected-error
    (and has-error? (hash-ref document 'expectError)))
  (when (and expected-error (not (string? expected-error)))
    (raise-ail "TEST_INVALID_ERROR_EXPECTATION"
               "expectError must be a diagnostic code string"
               (hasheq 'name name)))
  (ail-test-case name
                 (map jsexpr->value arguments)
                 (and has-expected? (hash-ref document 'expect))
                 expected-error))

(define (run-test-suite program suite
                        #:fuel-per-case [fuel-per-case 10000]
                        #:max-depth [maximum-depth 256])
  (define failures '())
  (define passed-count 0)
  (for ([test-case (in-list (ail-test-suite-cases suite))])
    (define failure
      (run-test-case program
                     (ail-test-suite-entry suite)
                     test-case
                     fuel-per-case
                     maximum-depth))
    (if failure
        (set! failures (cons failure failures))
        (set! passed-count (add1 passed-count))))
  (define total (length (ail-test-suite-cases suite)))
  (hasheq 'passed (null? failures)
          'total total
          'passedCount passed-count
          'failedCount (length failures)
          'failures (reverse failures)))

(define (run-test-case program entry test-case fuel maximum-depth)
  (with-handlers
      ([exn:fail:ail?
        (lambda (error)
          (if (and (ail-test-case-expected-error test-case)
                   (string=? (ail-test-case-expected-error test-case)
                             (exn:fail:ail-code error)))
              #f
              (hasheq 'name (ail-test-case-name test-case)
                      'reason "unexpected-error"
                      'expected
                      (or (ail-test-case-expected-error test-case)
                          (ail-test-case-expected test-case))
                      'actual (hash-ref (ail-error->jsexpr error) 'error))))])
    (define actual
      (value->jsexpr
       (execute-export program
                       entry
                       (ail-test-case-arguments test-case)
                       #:fuel fuel
                       #:max-depth maximum-depth
                       #:logger void)))
    (cond
      [(ail-test-case-expected-error test-case)
       (hasheq 'name (ail-test-case-name test-case)
               'reason "expected-error-not-raised"
               'expectedError (ail-test-case-expected-error test-case)
               'actual actual)]
      [(equal? actual (ail-test-case-expected test-case)) #f]
      [else
       (hasheq 'name (ail-test-case-name test-case)
               'reason "value-mismatch"
               'expected (ail-test-case-expected test-case)
               'actual actual)])))

(define (required-key document key)
  (hash-ref
   document
   key
   (lambda ()
     (raise-ail "TEST_MISSING_FIELD"
                "test document is missing a required field"
                (hasheq 'field (symbol->string key))))))

