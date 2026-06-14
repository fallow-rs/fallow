"use client";

interface CustomFormProps {
  action: (formData: FormData) => Promise<void>;
}

// A client component that accepts a server action via an `action` prop. The
// `action={propAction}` binding at the call site credits propAction as used.
export function CustomForm({ action }: CustomFormProps) {
  return <form action={action}>Submit</form>;
}
