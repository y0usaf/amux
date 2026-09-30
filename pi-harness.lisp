(defpackage #:pi-harness
  (:use #:cl #:ekko/extensions))
(in-package #:pi-harness)

(defparameter *env* "@env@")
(defparameter *pi* "@pi@")
(defparameter *status* "@status@")
(defparameter *hints* '("C-n new   C-S-r resume" "C-, C-. move   C-1..9" "C-S-w close  C-S-d detach"))

(defun session-argv (directory &rest arguments)
  (append (when directory (list *env* "-C" directory))
          (list *pi* "-e" *status*)
          arguments))

(defun pane-id (pane) (getf pane :id))

(defun directory-of (pane)
  (let ((argv (getf pane :argv)))
    (when (and (equal (first argv) *env*) (equal (second argv) "-C"))
      (third argv))))

(defun base-name (path)
  (let ((path (string-right-trim "/" path)))
    (subseq path (1+ (or (position #\/ path :from-end t) -1)))))

(defun split-title (title)
  (loop with start = 0
        for end = (search " - " title :start2 start)
        collect (subseq title start end)
        while end
        do (setf start (+ end 3))))

(defun busy-glyph-p (char)
  (or (<= #x2800 (char-code char) #x28ff) (find char "◐◑◒◓")))

(defun title-parts (pane)
  (let* ((title (or (getf pane :terminal-title) ""))
         (busy (and (> (length title) 2) (char= (char title 1) #\Space)
                    (busy-glyph-p (char title 0))))
         (parts (split-title (if busy (subseq title 2) title))))
    (values busy
            (when (> (length parts) 2) (format nil "~{~A~^ - ~}" (butlast (rest parts))))
            (when (> (length parts) 1) (first (last parts))))))

(defun project (pane)
  (let ((directory (directory-of pane)))
    (if directory
        (values directory (base-name directory))
        (let ((cwd (nth-value 2 (title-parts pane))))
          (values (or cwd "") (or cwd "…"))))))

(defun groups (panes)
  (let ((groups '()))
    (dolist (pane (sort (copy-list panes) #'< :key #'pane-id))
      (multiple-value-bind (key name) (project pane)
        (let ((group (assoc key groups :test #'equal)))
          (if group
              (nconc group (list pane))
              (push (list key name pane) groups)))))
    (nreverse groups)))

(defun ordered (panes)
  (loop for group in (groups panes) append (cddr group)))

(defun focused (snapshot)
  (find (value snapshot :focus) (value snapshot :panes) :key #'pane-id))

(defun focus-on (pane)
  (when pane (list (action :focus :pane (pane-id pane)))))

(defun move-focus (snapshot delta)
  (let* ((panes (ordered (value snapshot :panes)))
         (index (or (position (value snapshot :focus) panes :key #'pane-id) 0)))
    (when panes (focus-on (nth (mod (+ index delta) (length panes)) panes)))))

(defun go-to (snapshot event)
  (let ((n (parse-integer (or (first (getf event :arguments)) "") :junk-allowed t)))
    (when (and n (plusp n))
      (focus-on (nth (1- n) (ordered (value snapshot :panes)))))))

(defun open-session (snapshot event &rest arguments)
  (let ((directory (or (first (getf event :arguments))
                       (let ((pane (focused snapshot))) (and pane (directory-of pane))))))
    (list (action :split :axis :columns :argv (apply #'session-argv directory arguments)))))

(defun sidebar-width (cols)
  (if (< cols 60) 0 (max 20 (min 36 (round (* cols 22) 100)))))

(defun arrange (snapshot event)
  (declare (ignore event))
  (let* ((viewport (value snapshot :viewport))
         (sidebar (sidebar-width (getf viewport :cols)))
         (x (if (plusp sidebar) (1+ sidebar) 0))
         (width (max 1 (min 500 (- (getf viewport :cols) x))))
         (height (max 1 (min 300 (getf viewport :rows))))
         (panes (value snapshot :panes))
         (shown (or (find (value snapshot :focus) panes :key #'pane-id) (first panes))))
    (list (action :place-panes :version 1 :camera '(0 0)
                  :placements (loop for pane in panes
                                    collect (list :pane (pane-id pane) :x x :y 0
                                                  :cols width :rows height
                                                  :outer (list x 0 width height)
                                                  :visible (eq pane shown)))))))

(defun clean (text)
  (substitute-if #\Space (lambda (c) (let ((code (char-code c))) (or (< code 32) (<= 127 code 159))))
                 text))

(defun fit (text width)
  (with-output-to-string (out)
    (let ((used 0))
      (loop for c across (clean text)
            for w = (display-width c)
            while (<= (+ used w) width)
            do (write-char c out) (incf used w))
      (loop repeat (- width used) do (write-char #\Space out)))))

(defun mark (pane unread)
  (cond ((getf pane :exit) (values "×" '(2)))
        ((member (pane-id pane) unread) (values "!" '(1 33)))
        ((title-parts pane) (values "◐" '(36)))
        ((getf pane :activity) (values "●" '(32)))
        (t (values " " '()))))

(defun sidebar-spans (snapshot width rows)
  (let* ((focus (value snapshot :focus))
         (unread (loop for n in (value snapshot :notifications)
                       unless (getf n :read) collect (getf n :pane)))
         (hints (if (< rows 12) '() *hints*))
         (limit (- rows (length hints) (if hints 1 0)))
         (spans (list (list :x width :y 0 :text "│" :sgr '(2) :rows rows)))
         (y 0) (index 0))
    (flet ((row (&rest pieces)
             (when (< y limit)
               (dolist (piece pieces) (push (list* :y y piece) spans)))
             (incf y)))
      (dolist (group (groups (value snapshot :panes)))
        (destructuring-bind (key name &rest panes) group
          (when (plusp y) (incf y))
          (row (list* :x 0 :text (fit (format nil " ~A" name) width) :sgr '(1)
                      :wheel-command "cycle"
                      (when (eql (position #\/ key) 0) (list :command "new" :arguments (list key)))))
          (dolist (pane panes)
            (incf index)
            (multiple-value-bind (glyph glyph-sgr) (mark pane unread)
              (let ((sgr (if (eql (pane-id pane) focus) '(7) '()))
                    (target (list :focus :pane (pane-id pane))))
                (row (list :x 0 :text (format nil " ~A ~A " (if (<= index 9) index " ") glyph)
                           :sgr (append sgr glyph-sgr) :action target :wheel-command "cycle")
                     (list :x 5 :text (fit (or (nth-value 1 (title-parts pane)) "untitled") (- width 5))
                           :sgr sgr :action target :wheel-command "cycle"))))))))
    (loop for hint in hints
          for y from (- rows (length hints))
          do (push (list :x 0 :y y :text (fit (format nil " ~A" hint) width) :sgr '(2)) spans))
    (nreverse spans)))

(defun sidebar (snapshot event)
  (declare (ignore event))
  (let* ((viewport (value snapshot :viewport))
         (width (sidebar-width (getf viewport :cols))))
    (list (action :decorate :spans (when (plusp width)
                                     (sidebar-spans snapshot width (getf viewport :rows)))))))

(register-component :id :pi-harness :reads '(:panes :focus :viewport :notifications)
                    :handler #'sidebar)
(register-layout-provider :component :pi-harness :name "pi-harness"
                          :reads '(:panes :focus :viewport :geometry) :handler #'arrange)
(register-keymap :component :pi-harness :name :harness :unbound :forward)

(loop for (name value) on (list :layout-provider "pi-harness" :initial-keymap :harness
                                :shell (session-argv nil) :pane-budget 64
                                :pane-insets '(0 0 0 0) :viewport-insets '(0 0 0 0)
                                :split-gaps '(0 0))
      by #'cddr
      do (set-option :component :pi-harness :name name :value value))

(loop for (name handler) on (list "new" (lambda (s e) (open-session s e))
                                  "resume" (lambda (s e) (open-session s e "--resume"))
                                  "next" (lambda (s e) (declare (ignore e)) (move-focus s 1))
                                  "previous" (lambda (s e) (declare (ignore e)) (move-focus s -1))
                                  "cycle" (lambda (s e) (move-focus s (getf e :direction 1)))
                                  "go" #'go-to
                                  "close" (lambda (s e) (declare (ignore s e)) (list (action :close)))
                                  "detach" (lambda (s e) (declare (ignore s e)) (list (action :detach))))
      by #'cddr
      do (register-command :component :pi-harness :name name :handler handler))

(loop for (key command) on '("C-n" "new" "C-R" "resume" "C-." "next" "C-," "previous"
                             "C-W" "close" "C-D" "detach")
      by #'cddr
      do (bind-key :component :pi-harness :map :harness :key key :command command))
(loop for n from 1 to 9
      do (bind-key :component :pi-harness :map :harness :key (format nil "C-~D" n)
                   :command "go" :arguments (list (princ-to-string n))))
