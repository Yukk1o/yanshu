#lang racket/base

(require json
         net/uri-codec
         racket/file
         racket/list
         racket/port
         racket/string
         racket/tcp
         "ast.rkt"
         "error.rkt"
         "runtime.rkt"
         "service.rkt"
         "version-store.rkt")

(provide (struct-out running-http-server)
         start-http-server
         stop-http-server!)

(struct running-http-server (host port listener custodian accept-thread)
  #:transparent)

(define (start-http-server program-loader
                           #:store [store #f]
                           #:host [host "127.0.0.1"]
                           #:port [port 0]
                           #:max-workers [maximum-workers 32]
                           #:request-timeout-seconds [request-timeout 10]
                           #:handler-timeout-seconds [handler-timeout 5]
                           #:max-header-bytes [maximum-header-bytes 65536]
                           #:max-body-bytes [maximum-body-bytes (* 1024 1024)]
                           #:fuel [fuel 25000]
                           #:max-depth [maximum-depth 256]
                           #:logger [logger void]
                           #:observer [observer void]
                           #:static-root [static-root #f])
  (unless (procedure? program-loader)
    (raise-argument-error 'start-http-server "procedure?" program-loader))
  (for ([value (in-list (list maximum-workers
                              request-timeout
                              handler-timeout
                              maximum-header-bytes
                              maximum-body-bytes
                              fuel
                              maximum-depth))])
    (unless (and (exact-integer? value) (positive? value))
      (raise-argument-error 'start-http-server "exact-positive-integer?" value)))
  (unless (and (exact-integer? port) (<= 0 port 65535))
    (raise-argument-error 'start-http-server "listen port from 0 through 65535" port))
  (define normalized-static-root
    (and static-root (simplify-path static-root #f)))
  (when (and normalized-static-root
             (not (directory-exists? normalized-static-root)))
    (raise-ail "HTTP_STATIC_ROOT_MISSING"
               "configured static root does not exist"
               (hasheq 'path (path->string normalized-static-root))))
  (define server-custodian (make-custodian))
  (define listener #f)
  (define accept-thread #f)
  (define actual-port #f)
  (parameterize ([current-custodian server-custodian])
    (set! listener (tcp-listen port 128 #t host))
    (define-values (_local-host listener-port _remote-host _remote-port)
      (tcp-addresses listener #t))
    (set! actual-port listener-port)
    (define worker-slots (make-semaphore maximum-workers))
    (set!
     accept-thread
     (thread
      (lambda ()
        (let accept-loop ()
          (with-handlers ([exn:fail? (lambda (_error) (void))])
            (define-values (input output) (tcp-accept listener))
            (semaphore-wait worker-slots)
            (thread
             (lambda ()
               (dynamic-wind
                 void
                 (lambda ()
                   (handle-connection
                    input
                    output
                    program-loader
                    store
                    request-timeout
                    handler-timeout
                    maximum-header-bytes
                    maximum-body-bytes
                    fuel
                    maximum-depth
                    logger
                    observer
                    normalized-static-root))
                 (lambda ()
                   (close-input-port input)
                   (close-output-port output)
                   (semaphore-post worker-slots)))))
            (accept-loop)))))))
  (running-http-server host
                       actual-port
                       listener
                       server-custodian
                       accept-thread))

(define (stop-http-server! server)
  (unless (running-http-server? server)
    (raise-argument-error 'stop-http-server! "running-http-server?" server))
  (custodian-shutdown-all (running-http-server-custodian server))
  (void))

(define (handle-connection input
                           output
                           program-loader
                           store
                           request-timeout
                           handler-timeout
                           maximum-header-bytes
                           maximum-body-bytes
                           fuel
                           maximum-depth
                           logger
                           observer
                           static-root)
  (define deadline (+ (current-inexact-milliseconds)
                      (* request-timeout 1000.0)))
  (with-handlers
      ([exn:fail:ail?
        (lambda (error)
          (safe-write-response
           output
           (protocol-error-response error)))]
       [exn:fail?
        (lambda (_error)
          (safe-write-response
           output
           (service-response
            500
            (hasheq 'content-type "application/json; charset=utf-8")
            (hasheq 'error
                    (hasheq 'code "INTERNAL_ERROR"
                            'message "request could not be completed")))))])
    (define request
      (read-http-request input
                         deadline
                         maximum-header-bytes
                         maximum-body-bytes))
    (define static-response
      (and static-root (lookup-static-response static-root request)))
    (if static-response
        (write-http-response output static-response)
        (handle-program-request output
                                request
                                program-loader
                                store
                                handler-timeout
                                fuel
                                maximum-depth
                                logger
                                observer))))

(define (handle-program-request output
                                request
                                program-loader
                                store
                                handler-timeout
                                fuel
                                maximum-depth
                                logger
                                observer)
  (define program
    (with-handlers ([exn:fail?
                     (lambda (_error)
                       (raise-ail "HTTP_SERVICE_UNAVAILABLE"
                                  "service program is unavailable"))])
      (program-loader)))
  (unless (ail-program? program)
    (raise-ail "HTTP_INVALID_PROGRAM_LOADER"
               "program loader did not return an AI-Evolve program"))
  (define started (current-inexact-milliseconds))
  (define result
    (handle-with-timeout
     handler-timeout
     (lambda ()
       (handle-service-request program
                               request
                               #:store store
                               #:fuel fuel
                               #:max-depth maximum-depth
                               #:logger logger))))
  (define elapsed
    (max 0 (inexact->exact
            (round (- (current-inexact-milliseconds) started)))))
  (with-handlers ([exn:fail? (lambda (_error) (void))])
    (observer
     (hasheq
      'programHash (source-hash (ail-program-source program))
      'method (service-request-method request)
      'handler (if (dispatch-result-handler result)
                   (symbol->string (dispatch-result-handler result))
                   #f)
      'status
      (service-response-status (dispatch-result-response result))
      'latencyBucket (latency-bucket elapsed)
      'diagnosticCode
      (and (dispatch-result-diagnostic result)
           (hash-ref
            (hash-ref (dispatch-result-diagnostic result) 'error)
            'code)))))
  (write-http-response output (dispatch-result-response result)))

(define (lookup-static-response static-root request)
  (and (string=? (service-request-method request) "GET")
       (let* ([path (service-request-path request)]
              [asset
               (hash-ref
                (hash "/" (cons "index.html" "text/html; charset=utf-8")
                      "/app.js" (cons "app.js" "text/javascript; charset=utf-8")
                      "/styles.css" (cons "styles.css" "text/css; charset=utf-8"))
                path
                #f)])
         (and asset
              (let ([file-path (build-path static-root (car asset))])
                (and (file-exists? file-path)
                     (service-response
                      200
                      (hasheq 'content-type (cdr asset)
                              'cache-control "no-store"
                              'x-content-type-options "nosniff")
                      (file->bytes file-path))))))))

(define (handle-with-timeout timeout-seconds callback)
  (define child-custodian (make-custodian))
  (define result-channel (make-channel))
  (parameterize ([current-custodian child-custodian])
    (thread
     (lambda ()
       (channel-put
        result-channel
        (with-handlers ([exn? (lambda (error) (vector 'error error))])
          (vector 'ok (callback)))))))
  (define outcome (sync/timeout timeout-seconds result-channel))
  (custodian-shutdown-all child-custodian)
  (cond
    [(not outcome)
     (dispatch-result
      (service-response
       504
       (hasheq 'content-type "application/json; charset=utf-8")
       (hasheq 'error
               (hasheq 'code "GATEWAY_TIMEOUT"
                       'message "handler exceeded its execution deadline")))
      (hasheq 'error
              (hasheq 'code "SERVICE_HANDLER_TIMEOUT"
                      'message "handler exceeded its execution deadline"
                      'details (hasheq)))
      #f)]
    [(eq? (vector-ref outcome 0) 'ok) (vector-ref outcome 1)]
    [else (raise (vector-ref outcome 1))]))

(define (read-http-request input deadline maximum-header-bytes maximum-body-bytes)
  (define request-line
    (read-http-line input deadline 8192 "HTTP_REQUEST_LINE_TOO_LARGE"))
  (when (eof-object? request-line)
    (raise-ail "HTTP_EMPTY_REQUEST" "connection contained no HTTP request"))
  (define request-parts (string-split request-line))
  (unless (= (length request-parts) 3)
    (raise-ail "HTTP_INVALID_REQUEST_LINE" "HTTP request line is malformed"))
  (define method (string-upcase (car request-parts)))
  (define target (cadr request-parts))
  (define version (caddr request-parts))
  (unless (member version '("HTTP/1.0" "HTTP/1.1"))
    (raise-ail "HTTP_VERSION_NOT_SUPPORTED"
               "only HTTP/1.0 and HTTP/1.1 are supported"))
  (unless (string-prefix? target "/")
    (raise-ail "HTTP_INVALID_TARGET"
               "request target must use origin form"))
  (define headers (read-http-headers input deadline maximum-header-bytes))
  (when (and (string=? version "HTTP/1.1")
             (not (hash-has-key? headers "host")))
    (raise-ail "HTTP_HOST_REQUIRED" "HTTP/1.1 request requires a Host header"))
  (when (hash-has-key? headers "transfer-encoding")
    (raise-ail "HTTP_TRANSFER_ENCODING_UNSUPPORTED"
               "request transfer encoding is not supported"))
  (define content-length
    (if (hash-has-key? headers "content-length")
        (let ([parsed (string->number (hash-ref headers "content-length"))])
          (unless (and (exact-nonnegative-integer? parsed)
                       (<= parsed maximum-body-bytes))
            (raise-ail "HTTP_INVALID_CONTENT_LENGTH"
                       "Content-Length is invalid or exceeds the body limit"
                       (hasheq 'limitBytes maximum-body-bytes)))
          parsed)
        0))
  (when (and (positive? content-length)
             (not (json-content-type?
                   (hash-ref headers "content-type" ""))))
    (raise-ail "HTTP_UNSUPPORTED_MEDIA_TYPE"
               "request body must use application/json"))
  (define body-bytes
    (if (zero? content-length)
        #""
        (read-http-bytes input deadline content-length)))
  (define body
    (if (zero? content-length)
        '()
        (with-handlers ([exn:fail?
                         (lambda (_error)
                           (raise-ail "HTTP_INVALID_JSON"
                                      "request body is not valid JSON"))])
          (jsexpr->value (bytes->jsexpr body-bytes)))))
  (define target-match
    (regexp-match #px"^([^?]*)(?:[?](.*))?$" target))
  (unless target-match
    (raise-ail "HTTP_INVALID_TARGET" "request target is malformed"))
  (define path (decode-request-path (cadr target-match)))
  (define query
    (decode-query (or (caddr target-match) "")))
  (service-request method path query headers body))

(define (read-http-headers input deadline maximum-bytes)
  (let loop ([total 0] [count 0] [headers (hash)])
    (when (>= count 100)
      (raise-ail "HTTP_TOO_MANY_HEADERS"
                 "request contains too many headers"))
    (define remaining (max 1 (- maximum-bytes total)))
    (define line
      (read-http-line input deadline remaining "HTTP_HEADERS_TOO_LARGE"))
    (when (eof-object? line)
      (raise-ail "HTTP_INCOMPLETE_HEADERS"
                 "connection ended before HTTP headers completed"))
    (define next-total (+ total (string-length line) 2))
    (when (> next-total maximum-bytes)
      (raise-ail "HTTP_HEADERS_TOO_LARGE"
                 "request headers exceeded the byte limit"
                 (hasheq 'limitBytes maximum-bytes)))
    (cond
      [(string=? line "") headers]
      [else
       (define match (regexp-match #px"^([A-Za-z0-9-]+):[ \t]*(.*)$" line))
       (unless match
         (raise-ail "HTTP_INVALID_HEADER" "request header is malformed"))
       (define name (string-downcase (cadr match)))
       (define value (string-trim (caddr match)))
       (when (hash-has-key? headers name)
         (raise-ail "HTTP_DUPLICATE_HEADER"
                    "duplicate request headers are not supported"
                    (hasheq 'header name)))
       (loop next-total (add1 count) (hash-set headers name value))])))

(define (read-http-line input deadline maximum-bytes too-large-code)
  (define output (open-output-bytes))
  (let loop ([count 0])
    (when (> count maximum-bytes)
      (raise-ail too-large-code "HTTP line exceeded its byte limit"
                 (hasheq 'limitBytes maximum-bytes)))
    (define byte (read-byte/deadline input deadline))
    (cond
      [(eof-object? byte)
       (if (zero? count) eof
           (raise-ail "HTTP_INCOMPLETE_LINE"
                      "connection ended inside an HTTP line"))]
      [(= byte 10)
       (define raw (get-output-bytes output))
       (define without-cr
         (if (and (positive? (bytes-length raw))
                  (= (bytes-ref raw (sub1 (bytes-length raw))) 13))
             (subbytes raw 0 (sub1 (bytes-length raw)))
             raw))
       (with-handlers ([exn:fail?
                        (lambda (_error)
                          (raise-ail "HTTP_INVALID_TEXT"
                                     "HTTP line is not valid UTF-8"))])
         (bytes->string/utf-8 without-cr))]
      [else
       (write-byte byte output)
       (loop (add1 count))])))

(define (read-http-bytes input deadline count)
  (define output (make-bytes count))
  (let loop ([offset 0])
    (if (= offset count)
        output
        (begin
          (unless (sync/timeout (remaining-seconds deadline) input)
            (raise-ail "HTTP_REQUEST_TIMEOUT"
                       "HTTP request exceeded its read deadline"))
          (let ([read-count
                 (read-bytes-avail! output input offset count)])
            (when (eof-object? read-count)
              (raise-ail "HTTP_INCOMPLETE_BODY"
                         "connection ended before the request body completed"))
            (loop (+ offset read-count)))))))

(define (read-byte/deadline input deadline)
  (unless (sync/timeout (remaining-seconds deadline) input)
    (raise-ail "HTTP_REQUEST_TIMEOUT"
               "HTTP request exceeded its read deadline"))
  (read-byte input))

(define (remaining-seconds deadline)
  (define remaining (/ (- deadline (current-inexact-milliseconds)) 1000.0))
  (if (positive? remaining) remaining 0))

(define (decode-request-path raw-path)
  (with-handlers ([exn:fail?
                   (lambda (_error)
                     (raise-ail "HTTP_INVALID_PATH_ENCODING"
                                "request path contains invalid escaping"))])
    (if (string=? raw-path "/")
        "/"
        (string-append
         "/"
         (string-join
          (for/list ([segment (in-list
                               (string-split (substring raw-path 1)
                                             "/"
                                             #:trim? #f))])
            (define decoded (uri-path-segment-decode segment))
            (when (string-contains? decoded "/")
              (raise-ail "HTTP_INVALID_PATH_ENCODING"
                         "encoded slash is not allowed inside a path segment"))
            decoded)
          "/")))))

(define (decode-query raw-query)
  (if (string=? raw-query "")
      (hash)
      (for/fold ([result (hash)])
                ([part (in-list (string-split raw-query "&" #:trim? #f))])
        (define match (regexp-match #px"^([^=]*)(?:=(.*))?$" part))
        (with-handlers ([exn:fail?
                         (lambda (_error)
                           (raise-ail "HTTP_INVALID_QUERY_ENCODING"
                                      "query string contains invalid escaping"))])
          (define key (form-urlencoded-decode (cadr match)))
          (define value (form-urlencoded-decode (or (caddr match) "")))
          (hash-set result key value)))))

(define (json-content-type? content-type)
  (regexp-match? #px"(?i:^application/json(?:[ ]*;|$))" content-type))

(define (protocol-error-response error)
  (define code (exn:fail:ail-code error))
  (define status
    (cond
      [(member code '("HTTP_REQUEST_TIMEOUT")) 408]
      [(member code '("HTTP_UNSUPPORTED_MEDIA_TYPE")) 415]
      [(member code '("HTTP_VERSION_NOT_SUPPORTED")) 505]
      [(member code '("HTTP_SERVICE_UNAVAILABLE")) 500]
      [(member code '("HTTP_INVALID_CONTENT_LENGTH"
                      "HTTP_HEADERS_TOO_LARGE"
                      "HTTP_REQUEST_LINE_TOO_LARGE"
                      "HTTP_TOO_MANY_HEADERS")) 413]
      [else 400]))
  (service-response
   status
   (hasheq 'content-type "application/json; charset=utf-8")
   (hasheq 'error
           (hasheq 'code code
                   'message (exn-message error)))))

(define (write-http-response output response)
  (define response-body (service-response-body response))
  (define body-bytes
    (if (bytes? response-body)
        response-body
        (jsexpr->bytes response-body)))
  (define headers
    (hash-set
     (hash-set (service-response-headers response)
               'content-length
               (number->string (bytes-length body-bytes)))
     'connection
     "close"))
  (display (format "HTTP/1.1 ~a ~a\r\n"
                   (service-response-status response)
                   (reason-phrase (service-response-status response)))
           output)
  (for ([name (in-list (sort (hash-keys headers) symbol<?))])
    (display (format "~a: ~a\r\n"
                     (header-display-name name)
                     (hash-ref headers name))
             output))
  (display "\r\n" output)
  (write-bytes body-bytes output)
  (flush-output output))

(define (safe-write-response output response)
  (with-handlers ([exn:fail? (lambda (_error) (void))])
    (write-http-response output response)))

(define (reason-phrase status)
  (hash-ref
   (hasheq 200 "OK"
           201 "Created"
           204 "No Content"
           400 "Bad Request"
           404 "Not Found"
           405 "Method Not Allowed"
           408 "Request Timeout"
           409 "Conflict"
           413 "Content Too Large"
           415 "Unsupported Media Type"
           500 "Internal Server Error"
           504 "Gateway Timeout"
           505 "HTTP Version Not Supported")
   status
   "Response"))

(define (header-display-name name)
  (string-join
   (map string-titlecase (string-split (symbol->string name) "-"))
   "-"))

(define (latency-bucket milliseconds)
  (cond
    [(< milliseconds 10) "lt-10ms"]
    [(< milliseconds 50) "10-49ms"]
    [(< milliseconds 200) "50-199ms"]
    [(< milliseconds 1000) "200-999ms"]
    [else "gte-1000ms"]))
