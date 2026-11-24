import { DomSanitizer, SafeHtml } from "@angular/platform-browser";

interface Comment {
  body: string;
}

export class CommentHtml {
  constructor(private readonly sanitizer: DomSanitizer) {}

  trust(comment: Comment): SafeHtml {
    return this.sanitizer.bypassSecurityTrustHtml(comment.body);
  }
}
