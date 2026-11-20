export default async function EditPostPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  return <h1>Edit {slug}</h1>;
}
