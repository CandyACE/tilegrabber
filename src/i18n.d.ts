import type zhCN from "./locales/zh-CN";

declare module "vue-i18n" {
  type MessageSchema = typeof zhCN;
  export interface DefineLocaleMessage extends MessageSchema {}
}
