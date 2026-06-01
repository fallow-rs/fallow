import {
  Module,
  type NestModule,
  type MiddlewareConsumer,
  type OnModuleInit,
  type OnModuleDestroy,
} from '@nestjs/common';

// Implements NestModule (configure) and two lifecycle interfaces.
// configure / onModuleInit / onModuleDestroy are framework-dispatched and
// must NOT surface as unused-class-member.
@Module({})
export class AppModule
  implements NestModule, OnModuleInit, OnModuleDestroy
{
  configure(consumer: MiddlewareConsumer): void {
    void consumer;
  }

  onModuleInit(): void {
    // framework-invoked lifecycle hook
  }

  // Sibling lifecycle hook the class does NOT declare in its `implements`
  // clause. Nest dispatches by duck-typed presence, so this must also be
  // credited (the all-five crediting behavior).
  onApplicationBootstrap(): void {
    // framework-invoked lifecycle hook
  }

  onModuleDestroy(): void {
    // framework-invoked lifecycle hook
  }

  unusedModuleHelper(): string {
    // Genuinely unused; not a lifecycle name and not called anywhere.
    return 'never called';
  }
}
