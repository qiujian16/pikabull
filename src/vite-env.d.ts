/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

declare module "plotly.js-dist-min" {
  const Plotly: any;
  export default Plotly;
}
