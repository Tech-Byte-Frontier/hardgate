export function handler(name: string): Response {
  return new Response(`hello ${name}`);
}
