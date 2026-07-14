export interface GreeterPort {
  greet(name: string): string;
}
export interface Deps {
  greeter: GreeterPort;
}
