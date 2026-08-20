#lang racket/base

(require json
         openssl/sha1
         racket/file
         racket/path
         "ast.rkt"
         "error.rkt"
         "runtime.rkt")

(provide source-hash
         register-candidate!
         promote!
         rollback!
         active-hash
         active-source
         version-source
         version-metadata)

(define (source-hash source)
  (bytes->lowercase-hex
   (sha256-bytes (open-input-bytes (string->bytes/utf-8 source)))))

(define (register-candidate! root source
                             #:parent [parent #f]
                             #:provider [provider "unknown"]
                             #:provider-metadata [provider-metadata (hasheq)]
                             #:report [report (hasheq 'passed #f)])
  (define program (load-program-source source))
  (define hash (source-hash source))
  (ensure-store-directories! root)
  (define source-path (version-source-path root hash))
  (define metadata-path (version-metadata-path root hash))
  (unless (file-exists? source-path)
    (write-text-file source-path source))
  (unless (file-exists? metadata-path)
    (write-json-file
     metadata-path
     (hasheq 'hash hash
             'parent (or parent (json-null))
             'program (symbol->string (yanshu-program-name program))
             'languageVersion (yanshu-program-version program)
             'provider provider
             'providerMetadata provider-metadata
             'registeredAt (current-seconds)
             'report report)))
  (append-event! root
                 (hasheq 'event "registered"
                         'hash hash
                         'parent (or parent (json-null))
                         'provider provider
                         'at (current-seconds)))
  hash)

(define (promote! root hash)
  (define metadata (version-metadata root hash))
  (define report (hash-ref metadata 'report (lambda () (hasheq))))
  (unless (hash-ref report 'passed (lambda () #f))
    (raise-yanshu "VERSION_TESTS_NOT_PASSED"
               "candidate cannot be promoted before its test report passes"
               (hasheq 'hash hash)))
  (define current (active-hash root))
  (define raw-parent (hash-ref metadata 'parent (lambda () (json-null))))
  (define parent (if (eq? raw-parent (json-null)) #f raw-parent))
  (unless (equal? parent current)
    (raise-yanshu "VERSION_PARENT_MISMATCH"
               "candidate parent is not the active version"
               (hasheq 'hash hash
                       'candidateParent (or parent (json-null))
                       'active (or current (json-null)))))
  (write-active-pointer! root hash)
  (append-event! root
                 (hasheq 'event "promoted"
                         'from (or current (json-null))
                         'to hash
                         'at (current-seconds)))
  hash)

(define (rollback! root)
  (define current (active-hash root))
  (unless current
    (raise-yanshu "VERSION_NO_ACTIVE"
               "version store has no active version"))
  (define metadata (version-metadata root current))
  (define raw-parent (hash-ref metadata 'parent (lambda () (json-null))))
  (when (eq? raw-parent (json-null))
    (raise-yanshu "VERSION_NO_PARENT"
               "active version has no parent to roll back to"
               (hasheq 'hash current)))
  (version-metadata root raw-parent)
  (write-active-pointer! root raw-parent)
  (append-event! root
                 (hasheq 'event "rolled-back"
                         'from current
                         'to raw-parent
                         'at (current-seconds)))
  raw-parent)

(define (active-hash root)
  (define pointer-path (build-path root "active.json"))
  (and (file-exists? pointer-path)
       (hash-ref (read-json-file pointer-path) 'active)))

(define (active-source root)
  (define hash (active-hash root))
  (unless hash
    (raise-yanshu "VERSION_NO_ACTIVE"
               "version store has no active version"))
  (version-source root hash))

(define (version-source root hash)
  (define path (version-source-path root hash))
  (unless (file-exists? path)
    (raise-yanshu "VERSION_UNKNOWN"
               "version source does not exist"
               (hasheq 'hash hash)))
  (file->string path))

(define (version-metadata root hash)
  (define path (version-metadata-path root hash))
  (unless (file-exists? path)
    (raise-yanshu "VERSION_UNKNOWN"
               "version metadata does not exist"
               (hasheq 'hash hash)))
  (read-json-file path))

(define (ensure-store-directories! root)
  (make-directory* root)
  (make-directory* (build-path root "versions"))
  (make-directory* (build-path root "metadata")))

(define (version-source-path root hash)
  (build-path root "versions" (string-append hash ".yan")))

(define (version-metadata-path root hash)
  (build-path root "metadata" (string-append hash ".json")))

(define (write-active-pointer! root hash)
  (ensure-store-directories! root)
  (define target (build-path root "active.json"))
  (define temporary
    (build-path root
                (format "active-~a-~a.tmp"
                        (current-seconds)
                        (random 1000000))))
  (write-json-file temporary (hasheq 'active hash))
  (rename-file-or-directory temporary target #t))

(define (write-text-file path content)
  (call-with-output-file path
    (lambda (output) (display content output))
    #:exists 'truncate/replace))

(define (write-json-file path value)
  (call-with-output-file path
    (lambda (output)
      (write-json value output)
      (newline output))
    #:exists 'truncate/replace))

(define (read-json-file path)
  (call-with-input-file path read-json))

(define (append-event! root event)
  (call-with-output-file (build-path root "events.jsonl")
    (lambda (output)
      (write-json event output)
      (newline output))
    #:exists 'append))

(define (bytes->lowercase-hex bytes)
  (apply
   string-append
   (for/list ([byte (in-bytes bytes)])
     (define digits (number->string byte 16))
     (if (= (string-length digits) 1)
         (string-append "0" digits)
         digits))))
