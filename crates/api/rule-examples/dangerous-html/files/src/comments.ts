interface Comment {
  author: string;
  body: string;
}

export const renderComment = (element: HTMLElement, comment: Comment) => {
  element.innerHTML = comment.body;
};
