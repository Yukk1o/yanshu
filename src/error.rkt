#lang racket/base

(provide (struct-out exn:fail:ail)
         raise-ail
         ail-error->jsexpr)

(struct exn:fail:ail exn:fail (code details) #:transparent)

(define (raise-ail code message [details (hasheq)])
  (raise
   (exn:fail:ail message
                 (current-continuation-marks)
                 code
                 details)))

(define (ail-error->jsexpr error)
  (hasheq 'ok #f
          'error
          (hasheq 'code (exn:fail:ail-code error)
                  'message (exn-message error)
                  'details (exn:fail:ail-details error))))

