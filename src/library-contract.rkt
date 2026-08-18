#lang racket/base

(require racket/string)

(provide (struct-out library-requirement)
         (struct-out library-operation-contract)
         (struct-out library-contract)
         maximum-library-count
         maximum-library-version
         valid-library-name?
         find-library-contract)

(struct library-requirement (name version) #:transparent)
(struct library-operation-contract
  (name minimum-arity maximum-arity argument-kinds result-kind cost)
  #:transparent)
(struct library-contract (name version operations) #:transparent)

(define maximum-library-count 32)
(define maximum-library-version 65535)

(define (valid-library-name? value)
  (and (symbol? value)
       (regexp-match? #px"^[a-z][a-z0-9-]{0,63}$"
                      (symbol->string value))))

(define (text-cost arguments)
  (define character-count
    (for/sum ([argument (in-list arguments)])
      (if (string? argument) (string-length argument) 0)))
  (+ 1 (quotient (+ character-count 63) 64)))

(define text-v1
  (library-contract
   'text
   1
   (hasheq
    'text/length
    (library-operation-contract 'text/length 1 1 '(String) 'Int text-cost)
    'text/starts-with?
    (library-operation-contract
     'text/starts-with? 2 2 '(String String) 'Bool text-cost)
    'text/ends-with?
    (library-operation-contract
     'text/ends-with? 2 2 '(String String) 'Bool text-cost)
    'text/contains?
    (library-operation-contract
     'text/contains? 2 2 '(String String) 'Bool text-cost)
    'text/replace
    (library-operation-contract
     'text/replace 3 3 '(String String String) 'String text-cost))))

(define contracts
  (hash (cons 'text 1) text-v1))

(define (find-library-contract name version)
  (hash-ref contracts (cons name version) #f))
