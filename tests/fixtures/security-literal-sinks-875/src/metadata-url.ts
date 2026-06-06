export async function readMetadata(): Promise<Response> {
  return fetch("http://169.254.169.254/latest/meta-data/iam/security-credentials/");
}
