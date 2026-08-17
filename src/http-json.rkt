#lang racket/base

(require json
         net/url
         racket/port
         racket/string
         "error.rkt")

(provide post-json)

(define maximum-response-bytes (* 4 1024 1024))
(define diagnostic-body-limit 2048)

(define (post-json endpoint headers document timeout-seconds)
  (unless (and (exact-integer? timeout-seconds)
               (positive? timeout-seconds))
    (raise-argument-error 'post-json "exact-positive-integer?" timeout-seconds))
  (define result-channel (make-channel))
  (define request-custodian (make-custodian))
  (parameterize ([current-custodian request-custodian])
    (thread
     (lambda ()
       (define outcome
         (with-handlers ([exn? (lambda (error) (vector 'error error))])
           (vector 'ok
                   (perform-request endpoint headers document))))
       (channel-put result-channel outcome))))
  (define outcome (sync/timeout timeout-seconds result-channel))
  (custodian-shutdown-all request-custodian)
  (unless outcome
    (raise-ail "PROVIDER_TIMEOUT"
               "LLM provider request exceeded its wall-clock timeout"
               (hasheq 'timeoutSeconds timeout-seconds)))
  (case (vector-ref outcome 0)
    [(ok) (vector-ref outcome 1)]
    [else
     (define error (vector-ref outcome 1))
     (if (exn:fail:ail? error)
         (raise error)
         (raise-ail "PROVIDER_NETWORK_ERROR"
                    "LLM provider request failed"))]))

(define (perform-request endpoint headers document)
  (define-values (status-line _response-headers input)
    (http-sendrecv/url
     (string->url endpoint)
     #:method #"POST"
     #:headers headers
     #:data (string->bytes/utf-8 (jsexpr->string document))))
  (define body
    (dynamic-wind
      void
      (lambda () (read-limited-bytes input maximum-response-bytes))
      (lambda () (close-input-port input))))
  (define status-code (parse-status-code status-line))
  (unless (and (>= status-code 200) (< status-code 300))
    (raise-ail "PROVIDER_HTTP_ERROR"
               "LLM provider returned a non-success HTTP status"
               (hasheq 'status status-code
                       'body
                       (truncate-for-diagnostic
                        body
                        (authorization-secrets headers)))))
  (with-handlers ([exn:fail?
                   (lambda (_error)
                     (raise-ail "PROVIDER_INVALID_HTTP_JSON"
                                "LLM provider returned invalid JSON"
                                (hasheq 'status status-code)))])
    (bytes->jsexpr body)))

(define (parse-status-code status-line)
  (define match
    (regexp-match #px#"^HTTP/[^ ]+[ ]+([0-9]{3})" status-line))
  (unless match
    (raise-ail "PROVIDER_INVALID_HTTP_STATUS"
               "LLM provider returned an unrecognized HTTP status line"))
  (string->number (bytes->string/utf-8 (cadr match))))

(define (read-limited-bytes input limit)
  (define output (open-output-bytes))
  (let loop ([total 0])
    (define chunk (read-bytes (min 8192 (add1 (- limit total))) input))
    (cond
      [(eof-object? chunk) (get-output-bytes output)]
      [else
       (define next-total (+ total (bytes-length chunk)))
       (when (> next-total limit)
         (raise-ail "PROVIDER_RESPONSE_TOO_LARGE"
                    "LLM provider response exceeded the byte limit"
                    (hasheq 'limitBytes limit)))
       (write-bytes chunk output)
       (loop next-total)])))

(define (authorization-secrets headers)
  (filter
   values
   (for/list ([header (in-list headers)])
     (define match
       (regexp-match #px"(?i:^Authorization:[ ]*Bearer[ ]+(.+)$)" header))
     (and match (string-trim (cadr match))))))

(define (truncate-for-diagnostic body secrets)
  (define text
    (bytes->string/utf-8 body #\uFFFD))
  (define redacted
    (for/fold ([current text]) ([secret (in-list secrets)])
      (if (string=? secret "")
          current
          (regexp-replace* (regexp (regexp-quote secret))
                           current
                           "[REDACTED]"))))
  (if (> (string-length redacted) diagnostic-body-limit)
      (string-append (substring redacted 0 diagnostic-body-limit) "...")
      redacted))
