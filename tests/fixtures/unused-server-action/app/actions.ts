"use server";

// Referenced via `<form action={formAction}>` in page.tsx (a JSX value binding,
// credited by oxc_semantic). Must NOT be flagged.
export async function formAction(formData: FormData) {
  await save(formData);
}

// Referenced via a plain import-and-call in page.tsx. Must NOT be flagged.
export async function callAction() {
  await ping();
}

// Referenced via a component prop `<CustomForm action={propAction} />`. Must
// NOT be flagged.
export async function propAction(formData: FormData) {
  await save(formData);
}

// Referenced by NO consumer anywhere. This is the dead server action.
export async function deadAction(formData: FormData) {
  await save(formData);
}

// Suppressed as unused-server-action: must appear in NEITHER bucket.
// fallow-ignore-next-line unused-server-action
export async function suppressedDeadAction() {
  await ping();
}

async function save(_data: FormData) {}
async function ping() {}
