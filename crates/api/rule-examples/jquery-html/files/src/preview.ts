import $ from "jquery";

interface Comment {
  body: string;
}

export const showPreview = (comment: Comment): void => {
  $("#preview").html(comment.body);
};
