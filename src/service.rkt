#lang racket/base

(require racket/list
         racket/string
         "ast.rkt"
         "error.rkt"
         "kv-store.rkt"
         "library-backend.rkt"
         "runtime.rkt")

(provide (struct-out service-request)
         (struct-out service-response)
         (struct-out dispatch-result)
         dispatch-request
         handle-service-request)

(struct service-request (method path query headers body) #:transparent)
(struct service-response (status headers body) #:transparent)
(struct dispatch-result (response diagnostic handler) #:transparent)

(define (handle-service-request program request
                                #:store [store #f]
                                #:fuel [fuel 25000]
                                #:max-depth [maximum-depth 256]
                                #:logger [logger void]
                                #:clock [clock current-milliseconds]
                                #:library-backends
                                [library-backends
                                 (make-reference-library-backends)]
                                #:capability-bindings
                                [extra-capability-bindings (hasheq)])
  (define clock-bindings
    (hasheq
     'clock
     (hasheq
      'now-ms
      (capability-primitive 0 0 (lambda (_arguments) (clock))))))
  (define base-bindings
    (merge-capability-bindings clock-bindings extra-capability-bindings))
  (with-handlers
      ([exn:fail:yanshu?
        (lambda (error)
          (internal-dispatch-result request error #f))]
       [exn:fail?
        (lambda (_error)
          (internal-dispatch-result
           request
           (exn:fail:yanshu "service host failed"
                         (current-continuation-marks)
                         "SERVICE_HOST_FAILURE"
                         (hasheq))
           #f))])
    (if store
        (call-with-kv-transaction
         store
         (lambda (kv-bindings commit!)
           (define result
             (dispatch-request
              program
              request
              #:fuel fuel
              #:max-depth maximum-depth
              #:logger logger
              #:library-backends library-backends
              #:capability-bindings
              (merge-capability-bindings base-bindings kv-bindings)))
           (unless (dispatch-result-diagnostic result) (commit!))
           result))
        (dispatch-request program
                          request
                          #:fuel fuel
                          #:max-depth maximum-depth
                          #:logger logger
                          #:library-backends library-backends
                          #:capability-bindings base-bindings))))

(define (dispatch-request program request
                          #:fuel [fuel 25000]
                          #:max-depth [maximum-depth 256]
                          #:logger [logger void]
                          #:library-backends
                          [library-backends
                           (make-reference-library-backends)]
                          #:capability-bindings [capability-bindings (hasheq)])
  (validate-service-request request)
  (define method (string-upcase (service-request-method request)))
  (define path (service-request-path request))
  (define path-matches
    (for/list ([route (in-list (yanshu-program-routes program))]
               #:do [(define parameters
                       (match-route-path (yanshu-route-path route) path))]
               #:when parameters)
      (cons route parameters)))
  (define selected
    (for/first ([match (in-list path-matches)]
                #:when (string=? method (yanshu-route-method (car match))))
      match))
  (cond
    [(not selected)
     (if (pair? path-matches)
         (dispatch-result
          (service-response
           405
           (hasheq 'content-type "application/json; charset=utf-8"
                   'allow
                   (string-join
                    (remove-duplicates
                     (map (lambda (match)
                            (yanshu-route-method (car match)))
                          path-matches))
                    ", "))
           (hasheq 'error
                   (hasheq 'code "METHOD_NOT_ALLOWED"
                           'message "method is not allowed for this path"
                           'details (hasheq))))
          #f
          #f)
         (dispatch-result
          (service-response
           404
           (hasheq 'content-type "application/json; charset=utf-8")
           (hasheq 'error
                   (hasheq 'code "ROUTE_NOT_FOUND"
                           'message "no route matches the request"
                           'details (hasheq))))
          #f
          #f))]
    [else
     (define route (car selected))
     (define parameters (cdr selected))
     (with-handlers
         ([exn:fail:yanshu?
           (lambda (error)
             (internal-dispatch-result
              request error (yanshu-route-handler route)))]
          [exn:fail?
           (lambda (_error)
             (internal-dispatch-result
              request
              (exn:fail:yanshu "guest handler host boundary failed"
                            (current-continuation-marks)
                            "SERVICE_HANDLER_FAILURE"
                            (hasheq))
              (yanshu-route-handler route)))])
       (define guest-request
         (hash "method" method
               "path" path
               "params" parameters
               "query" (service-request-query request)
               "headers" (service-request-headers request)
               "body" (service-request-body request)))
       (define raw-response
         (execute-export program
                         (yanshu-route-handler route)
                         (list guest-request)
                         #:fuel fuel
                         #:max-depth maximum-depth
                         #:logger logger
                         #:library-backends library-backends
                         #:capability-bindings capability-bindings))
       (dispatch-result (validate-guest-response raw-response)
                        #f
                        (yanshu-route-handler route)))]))

(define (validate-service-request request)
  (unless (service-request? request)
    (raise-argument-error 'dispatch-request "service-request?" request))
  (unless (and (string? (service-request-method request))
               (string? (service-request-path request))
               (string-prefix? (service-request-path request) "/")
               (hash? (service-request-query request))
               (hash? (service-request-headers request)))
    (raise-yanshu "SERVICE_INVALID_REQUEST"
               "service request has an invalid shape")))

(define (validate-guest-response value)
  (unless (and (hash? value) (= (hash-count value) 3))
    (raise-yanshu "SERVICE_INVALID_RESPONSE"
               "handler response must contain status, headers, and body"))
  (define status (compatible-hash-ref value "status"))
  (define headers (compatible-hash-ref value "headers"))
  (define body (compatible-hash-ref value "body"))
  (unless (and (exact-integer? status) (<= 100 status 599))
    (raise-yanshu "SERVICE_INVALID_RESPONSE_STATUS"
               "handler response status must be an integer from 100 through 599"))
  (unless (hash? headers)
    (raise-yanshu "SERVICE_INVALID_RESPONSE_HEADERS"
               "handler response headers must be a map"))
  (define normalized-headers
    (for/hasheq ([(key item) (in-hash headers)])
      (define key-string
        (cond
          [(string? key) key]
          [(symbol? key) (symbol->string key)]
          [else
           (raise-yanshu "SERVICE_INVALID_RESPONSE_HEADERS"
                      "response header names must be strings")]))
      (unless (regexp-match? #px"^[A-Za-z0-9-]+$" key-string)
        (raise-yanshu "SERVICE_INVALID_RESPONSE_HEADERS"
                   "response header name contains invalid characters"))
      (unless (string? item)
        (raise-yanshu "SERVICE_INVALID_RESPONSE_HEADERS"
                   "response header values must be strings"
                   (hasheq 'header key-string)))
      (when (or (string-contains? item "\r")
                (string-contains? item "\n"))
        (raise-yanshu "SERVICE_INVALID_RESPONSE_HEADERS"
                   "response header value contains a line break"
                   (hasheq 'header key-string)))
      (values (string->symbol (string-downcase key-string)) item)))
  (service-response
   status
   (if (hash-has-key? normalized-headers 'content-type)
       normalized-headers
       (hash-set normalized-headers
                 'content-type
                 "application/json; charset=utf-8"))
   (value->jsexpr body)))

(define (compatible-hash-ref mapping key)
  (cond
    [(hash-has-key? mapping key) (hash-ref mapping key)]
    [(hash-has-key? mapping (string->symbol key))
     (hash-ref mapping (string->symbol key))]
    [else
     (raise-yanshu "SERVICE_INVALID_RESPONSE"
                "handler response is missing a required field"
                (hasheq 'field key))]))

(define (match-route-path pattern path)
  (define pattern-segments (path-segments pattern))
  (define actual-segments (path-segments path))
  (and (= (length pattern-segments) (length actual-segments))
       (let loop ([patterns pattern-segments]
                  [actuals actual-segments]
                  [parameters (hash)])
         (cond
           [(null? patterns) parameters]
           [(string-prefix? (car patterns) ":")
            (loop (cdr patterns)
                  (cdr actuals)
                  (hash-set parameters
                            (substring (car patterns) 1)
                            (car actuals)))]
           [(string=? (car patterns) (car actuals))
            (loop (cdr patterns) (cdr actuals) parameters)]
           [else #f]))))

(define (path-segments path)
  (if (string=? path "/")
      '()
      (string-split (substring path 1) "/" #:trim? #f)))

(define (internal-dispatch-result request error handler)
  (define request-id
    (format "req-~a-~a" (current-milliseconds) (random 1000000)))
  (dispatch-result
   (service-response
    500
    (hasheq 'content-type "application/json; charset=utf-8")
    (hasheq 'error
            (hasheq 'code "INTERNAL_ERROR"
                    'message "request could not be completed"
                    'details (hasheq 'requestId request-id))))
   (hasheq 'requestId request-id
           'method (and (service-request? request)
                        (service-request-method request))
           'path (and (service-request? request)
                      (service-request-path request))
           'handler (if handler (symbol->string handler) #f)
           'error (hash-ref (yanshu-error->jsexpr error) 'error))
   handler))

(define (merge-capability-bindings left right)
  (for/fold ([result left]) ([(key value) (in-hash right)])
    (hash-set result key value)))
