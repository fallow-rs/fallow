import { AppModule } from './nest-app-module';
import { AuthGuard } from './nest-auth-guard';
import { PlainService } from './plain-service';

// Reference the classes so they are reachable exports; their members are not
// statically called, which is exactly the framework-dispatch scenario.
const modules = [AppModule, AuthGuard, PlainService];
for (const ctor of modules) {
  new ctor();
}
