#lang racket/base

(provide (struct-out exn:fail:yanshu)
         raise-yanshu
         yanshu-error->jsexpr)

(struct exn:fail:yanshu exn:fail (code details) #:transparent)

(define (raise-yanshu code message [details (hasheq)])
  (raise
   (exn:fail:yanshu message
                 (current-continuation-marks)
                 code
                 details)))

(define (yanshu-error->jsexpr error)
  (hasheq 'ok #f
          'error
          (hasheq 'code (exn:fail:yanshu-code error)
                  'message (exn-message error)
                  'details (exn:fail:yanshu-details error))))

