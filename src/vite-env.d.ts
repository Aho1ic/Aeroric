/// <reference types="vite/client" />

declare module "shiki/dist/langs/*.mjs" {
  import type { LanguageRegistration, MaybeArray } from "shiki/types";

  const lang: MaybeArray<LanguageRegistration>;
  export default lang;
}

declare module "shiki/dist/themes/*.mjs" {
  import type { ThemeRegistrationAny } from "shiki/types";

  const theme: ThemeRegistrationAny;
  export default theme;
}
