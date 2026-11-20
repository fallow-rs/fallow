export default async function PostPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return <h1>Post {id}</h1>;
}
