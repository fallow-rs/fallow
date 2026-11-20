"use server";

export async function placeOrder(form: FormData): Promise<void> {
  console.log("order", form.get("cartId"));
}

export async function submitForm(form: FormData): Promise<void> {
  console.log("feedback", form.get("message"));
}
