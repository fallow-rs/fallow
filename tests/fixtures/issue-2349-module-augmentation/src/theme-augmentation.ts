type AppTheme = {
  brand: string;
};

declare module './library' {
  export interface Theme extends AppTheme {}
}

export type UnusedLocalAlias = string;

export {};
