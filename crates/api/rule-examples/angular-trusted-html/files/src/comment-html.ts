import { DomSanitizer, SafeHtml } from "@angular/platform-browser";

interface Comment {
  body: string;
}

export const trustCommentHtml = (sanitizer: DomSanitizer, comment: Comment): SafeHtml =>
  sanitizer.bypassSecurityTrustHtml(comment.body);
