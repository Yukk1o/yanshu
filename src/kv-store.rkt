#lang racket/base

(require json
         racket/file
         racket/list
         racket/path
         racket/string
         "error.rkt"
         "runtime.rkt")

(provide kv-store?
         make-memory-kv-store
         open-file-kv-store
         call-with-kv-transaction
         kv-store-snapshot)

(struct kv-store (path data lock) #:mutable #:transparent)

(define (make-memory-kv-store)
  (kv-store #f (hash) (make-semaphore 1)))

(define (open-file-kv-store path)
  (define normalized (simplify-path path #f))
  (define data
    (if (file-exists? normalized)
        (read-store-file normalized)
        (hash)))
  (kv-store normalized data (make-semaphore 1)))

(define (kv-store-snapshot store)
  (unless (kv-store? store)
    (raise-argument-error 'kv-store-snapshot "kv-store?" store))
  (with-store-lock
   store
   (lambda ()
     (for/hash ([(key value) (in-hash (kv-store-data store))])
       (values key (copy-guest-value value))))))

(define (call-with-kv-transaction store callback)
  (unless (kv-store? store)
    (raise-argument-error 'call-with-kv-transaction "kv-store?" store))
  (unless (procedure? callback)
    (raise-argument-error 'call-with-kv-transaction "procedure?" callback))
  (with-store-lock
   store
   (lambda ()
     (define working
       (make-hash (hash->list (kv-store-data store))))
     (define commit-requested? #f)
     (define (commit!) (set! commit-requested? #t))
     (define bindings
       (hasheq
        'kv
        (hasheq
         'kv-get
         (capability-primitive
          2 2
          (lambda (arguments)
            (define key (expect-kv-key 'kv-get (car arguments)))
            (copy-guest-value
             (hash-ref working key (lambda () (cadr arguments))))))
         'kv-put
         (capability-primitive
          2 2
          (lambda (arguments)
            (define key (expect-kv-key 'kv-put (car arguments)))
            (define value (copy-guest-value (cadr arguments)))
            (hash-set! working key value)
            (copy-guest-value value)))
         'kv-delete
         (capability-primitive
          1 1
          (lambda (arguments)
            (define key (expect-kv-key 'kv-delete (car arguments)))
            (define existed? (hash-has-key? working key))
            (hash-remove! working key)
            existed?))
         'kv-list
         (capability-primitive
          1 1
          (lambda (arguments)
            (define prefix (expect-kv-key 'kv-list (car arguments)))
            (for/list ([key (in-list (sort (hash-keys working) string<?))]
                       #:when (string-prefix? key prefix))
              (copy-guest-value (hash-ref working key))))))))
     (define results
       (call-with-values
        (lambda () (callback bindings commit!))
        list))
     (when commit-requested?
       (define immutable-working
         (for/hash ([(key value) (in-hash working)])
           (values key value)))
       (when (kv-store-path store)
         (write-store-file! (kv-store-path store) immutable-working))
       (set-kv-store-data! store immutable-working))
     (apply values results))))

(define (with-store-lock store callback)
  (semaphore-wait (kv-store-lock store))
  (dynamic-wind
    void
    callback
    (lambda () (semaphore-post (kv-store-lock store)))))

(define (expect-kv-key operation value)
  (unless (and (string? value)
               (positive? (string-length value))
               (<= (string-length value) 512)
               (not (for/or ([character (in-string value)])
                      (char=? character #\nul))))
    (raise-yanshu "KV_INVALID_KEY"
               "KV key must be a non-empty bounded string"
               (hasheq 'operation (symbol->string operation))))
  value)

(define (copy-guest-value value)
  (jsexpr->value (value->jsexpr value)))

(define (read-store-file path)
  (define document
    (with-handlers ([exn:fail?
                     (lambda (_error)
                       (raise-yanshu "KV_INVALID_FILE"
                                  "KV persistence file is not valid JSON"
                                  (hasheq 'path (path->string path))))])
      (call-with-input-file path read-json)))
  (unless (and (hash? document)
               (equal? (hash-ref document 'version #f) 1)
               (list? (hash-ref document 'entries #f)))
    (raise-yanshu "KV_INVALID_FILE"
               "KV persistence file has an invalid document shape"
               (hasheq 'path (path->string path))))
  (for/fold ([result (hash)])
            ([entry (in-list (hash-ref document 'entries))])
    (unless (and (hash? entry)
                 (string? (hash-ref entry 'key #f))
                 (hash-has-key? entry 'value))
      (raise-yanshu "KV_INVALID_FILE"
                 "KV persistence entry is malformed"
                 (hasheq 'path (path->string path))))
    (define key (expect-kv-key 'load (hash-ref entry 'key)))
    (when (hash-has-key? result key)
      (raise-yanshu "KV_INVALID_FILE"
                 "KV persistence file contains a duplicate key"
                 (hasheq 'path (path->string path) 'key key)))
    (hash-set result key (jsexpr->value (hash-ref entry 'value)))))

(define (write-store-file! path data)
  (define parent (path-only path))
  (when parent (make-directory* parent))
  (define temporary
    (build-path (or parent (current-directory))
                (format ".~a-~a-~a.tmp"
                        (path->string (file-name-from-path path))
                        (current-seconds)
                        (random 1000000))))
  (dynamic-wind
    void
    (lambda ()
      (call-with-output-file temporary
        (lambda (output)
          (write-json
           (hasheq
            'version 1
            'entries
            (for/list ([key (in-list (sort (hash-keys data) string<?))])
              (hasheq 'key key
                      'value (value->jsexpr (hash-ref data key)))))
           output)
          (newline output)
          (flush-output output))
        #:exists 'error)
      (rename-file-or-directory temporary path #t))
    (lambda ()
      (when (file-exists? temporary)
        (delete-file temporary)))))

