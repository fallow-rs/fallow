// Expression body member access: import('./x').then(m => m.foo)
const result1 = import('./lib').then(m => m.foo);

// Destructured parameter: import('./x').then(({ bar, baz }) => ...)
import('./lib').then(({ bar, baz }) => {
  console.log(bar, baz);
});

// Angular routing pattern with .then(m => m.Component)
const routes = [
  {
    path: 'dashboard',
    loadComponent: () => import('./dashboard.component').then(m => m.DashboardComponent),
  },
  {
    path: 'settings',
    loadComponent: () => import('./settings.component').then(m => m.SettingsComponent),
  },
];

// Call on the callback param inside a guarded block
declare const enabled: boolean;
if (enabled) {
  void import('./inspector').then((m) => m.startInspector({ url: 'http://localhost' }));
}

// The callback param escapes, so the whole module is used
declare function register(value: unknown): void;
void import('./escaped').then((mod) => register(mod));

export { result1, routes };
