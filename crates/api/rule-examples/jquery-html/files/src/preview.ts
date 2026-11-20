import $ from "jquery";

interface Comment {
  body: string;
}

export const showPreview = (comment: Comment): void => {
  const preview = $("#preview");
  preview.html(comment.body);
};
