import {
  Injectable,
  type CanActivate,
  type ExecutionContext,
} from '@nestjs/common';

// canActivate is the guard dispatch method (CanActivate). Must NOT be flagged.
@Injectable()
export class AuthGuard implements CanActivate {
  canActivate(context: ExecutionContext): boolean {
    void context;
    return true;
  }

  unusedGuardHelper(): void {
    // Genuinely unused; should still be reported.
  }
}
