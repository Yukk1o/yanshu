#lang racket/base

(require json
         racket/file
         "error.rkt"
         "runtime.rkt")

(provide (struct-out yanshu-test-suite)
         (struct-out yanshu-test-case)
         load-test-suite
         run-test-suite)

(struct yanshu-test-suite (entry cases) #:transparent)
(struct yanshu-test-case (name arguments expected expected-error) #:transparent)

(define (load-test-suite path)
  (define document
    (call-with-input-file path read-json))
  (unless (hash? document)
    (raise-yanshu "TEST_INVALID_DOCUMENT"
               "test suite must be a JSON object"))
  (define entry-value (required-key document 'entry))
  (unless (string? entry-value)
    (raise-yanshu "TEST_INVALID_ENTRY"
               "test suite entry must be a string"))
  (define case-values (required-key document 'cases))
  (unless (list? case-values)
    (raise-yanshu "TEST_INVALID_CASES"
               "test suite cases must be a JSON array"))
  (yanshu-test-suite
   (string->symbol entry-value)
   (for/list ([case-value (in-list case-values)]
              [index (in-naturals)])
     (parse-test-case case-value index))))

(define (parse-test-case document index)
  (unless (hash? document)
    (raise-yanshu "TEST_INVALID_CASE"
               "test case must be a JSON object"
               (hasheq 'index index)))
  (define name (hash-ref document 'name (lambda () (format "case-~a" index))))
  (unless (string? name)
    (raise-yanshu "TEST_INVALID_CASE_NAME"
               "test case name must be a string"
               (hasheq 'index index)))
  (define arguments (required-key document 'args))
  (unless (list? arguments)
    (raise-yanshu "TEST_INVALID_ARGUMENTS"
               "test case args must be a JSON array"
               (hasheq 'name name)))
  (define has-expected? (hash-has-key? document 'expect))
  (define has-error? (hash-has-key? document 'expectError))
  (unless (or has-expected? has-error?)
    (raise-yanshu "TEST_MISSING_EXPECTATION"
               "test case must define expect or expectError"
               (hasheq 'name name)))
  (when (and has-expected? has-error?)
    (raise-yanshu "TEST_AMBIGUOUS_EXPECTATION"
               "test case cannot define both expect and expectError"
               (hasheq 'name name)))
  (define expected-error
    (and has-error? (hash-ref document 'expectError)))
  (when (and expected-error (not (string? expected-error)))
    (raise-yanshu "TEST_INVALID_ERROR_EXPECTATION"
               "expectError must be a diagnostic code string"
               (hasheq 'name name)))
  (yanshu-test-case name
                 (map jsexpr->value arguments)
                 (and has-expected? (hash-ref document 'expect))
                 expected-error))

(define (run-test-suite program suite
                        #:fuel-per-case [fuel-per-case 10000]
                        #:max-depth [maximum-depth 256])
  (define failures '())
  (define passed-count 0)
  (for ([test-case (in-list (yanshu-test-suite-cases suite))])
    (define failure
      (run-test-case program
                     (yanshu-test-suite-entry suite)
                     test-case
                     fuel-per-case
                     maximum-depth))
    (if failure
        (set! failures (cons failure failures))
        (set! passed-count (add1 passed-count))))
  (define total (length (yanshu-test-suite-cases suite)))
  (hasheq 'passed (null? failures)
          'total total
          'passedCount passed-count
          'failedCount (length failures)
          'failures (reverse failures)))

(define (run-test-case program entry test-case fuel maximum-depth)
  (with-handlers
      ([exn:fail:yanshu?
        (lambda (error)
          (if (and (yanshu-test-case-expected-error test-case)
                   (string=? (yanshu-test-case-expected-error test-case)
                             (exn:fail:yanshu-code error)))
              #f
              (hasheq 'name (yanshu-test-case-name test-case)
                      'reason "unexpected-error"
                      'expected
                      (or (yanshu-test-case-expected-error test-case)
                          (yanshu-test-case-expected test-case))
                      'actual (hash-ref (yanshu-error->jsexpr error) 'error))))])
    (define actual
      (value->jsexpr
       (execute-export program
                       entry
                       (yanshu-test-case-arguments test-case)
                       #:fuel fuel
                       #:max-depth maximum-depth
                       #:logger void)))
    (cond
      [(yanshu-test-case-expected-error test-case)
       (hasheq 'name (yanshu-test-case-name test-case)
               'reason "expected-error-not-raised"
               'expectedError (yanshu-test-case-expected-error test-case)
               'actual actual)]
      [(equal? actual (yanshu-test-case-expected test-case)) #f]
      [else
       (hasheq 'name (yanshu-test-case-name test-case)
               'reason "value-mismatch"
               'expected (yanshu-test-case-expected test-case)
               'actual actual)])))

(define (required-key document key)
  (hash-ref
   document
   key
   (lambda ()
     (raise-yanshu "TEST_MISSING_FIELD"
                "test document is missing a required field"
                (hasheq 'field (symbol->string key))))))

