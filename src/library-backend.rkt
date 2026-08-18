#lang racket/base

(require racket/string)

(provide (struct-out library-backend)
         make-reference-library-backends)

(struct library-backend (name version provider implementations) #:transparent)

(define text-v1-backend
  (library-backend
   'text
   1
   "racket-reference"
   (hasheq
    'text/length
    (lambda (arguments)
      (string-length (car arguments)))
    'text/starts-with?
    (lambda (arguments)
      (string-prefix? (car arguments) (cadr arguments)))
    'text/ends-with?
    (lambda (arguments)
      (string-suffix? (car arguments) (cadr arguments)))
    'text/contains?
    (lambda (arguments)
      (if (string-contains? (car arguments) (cadr arguments)) #t #f))
    'text/replace
    (lambda (arguments)
      (string-replace (car arguments)
                      (cadr arguments)
                      (caddr arguments))))))

(define (make-reference-library-backends)
  (hash (cons 'text 1) text-v1-backend))
