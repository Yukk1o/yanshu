#lang racket/base

(require json
         racket/file
         racket/list
         "version-store.rkt")

(provide run-version-store-scenario)

(define (run-version-store-scenario initial-path candidate-path)
  (define initial-source (file->string initial-path))
  (define candidate-source (file->string candidate-path))
  (define root (make-temporary-file "ai-lang-version-scenario-~a" 'directory))
  (dynamic-wind
    void
    (lambda ()
      (define report (hasheq 'passed #t))
      (define initial-hash
        (register-candidate! root
                             initial-source
                             #:provider "version-conformance"
                             #:provider-metadata (hasheq)
                             #:report report))
      (promote! root initial-hash)
      (define candidate-hash
        (register-candidate! root
                             candidate-source
                             #:parent initial-hash
                             #:provider "version-conformance"
                             #:provider-metadata (hasheq)
                             #:report report))
      (promote! root candidate-hash)
      (define promoted-active (active-hash root))
      (define metadata
        (list (normalize-metadata (version-metadata root initial-hash))
              (normalize-metadata (version-metadata root candidate-hash))))
      (rollback! root)
      (define rollback-active (active-hash root))
      (define active-source-hash (source-hash (active-source root)))
      (define events (read-event-names (build-path root "events.jsonl")))
      (define passed
        (and (equal? promoted-active candidate-hash)
             (equal? rollback-active initial-hash)
             (equal? active-source-hash initial-hash)
             (equal? events
                     '("registered" "promoted" "registered" "promoted"
                       "rolled-back"))))
      (hasheq 'formatVersion 1
              'passed passed
              'initialHash initial-hash
              'candidateHash candidate-hash
              'promotedActive promoted-active
              'rollbackActive rollback-active
              'activeSourceHash active-source-hash
              'metadata metadata
              'events events))
    (lambda () (delete-directory/files root))))

(define metadata-keys
  '(hash parent program languageVersion provider providerMetadata report))

(define (normalize-metadata metadata)
  (for/hasheq ([key (in-list metadata-keys)])
    (values key (hash-ref metadata key))))

(define (read-event-names path)
  (call-with-input-file
   path
   (lambda (input)
     (let loop ([result '()])
       (define event (read-json input))
       (if (eof-object? event)
           (reverse result)
           (loop (cons (hash-ref event 'event) result)))))))
