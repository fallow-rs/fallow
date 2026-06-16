// A PLAIN module (no file-level "use server"). Each exported function / arrow
// below carries an INLINE `"use server"` body directive, so it is a Server
// Action even though the file is not a use-server file. A dead one must be
// reclassified from `unused-export` to `unused-server-action` (W4.4); a used
// one must not be flagged.

// Referenced (import-and-call) from page.tsx. Must NOT be flagged.
export async function usedInlineAction() {
  "use server";
  await persist();
}

// Referenced by NO consumer. A dead inline Server Action declared as a function:
// must be reclassified out of unused-export into unused-server-action.
export async function deadInlineAction(formData: FormData) {
  "use server";
  await persist(formData);
}

// Referenced by NO consumer. A dead inline Server Action declared as a const
// arrow: must be reclassified the same way (covers the const-arrow capture).
export const deadInlineArrow = async (formData: FormData) => {
  "use server";
  await persist(formData);
};

async function persist(_data?: FormData) {}
