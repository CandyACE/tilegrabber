import { ref } from "vue";

export type ToastType = "success" | "warning" | "error" | "info";

export interface Toast {
  id: number;
  type: ToastType;
  title: string;
  message?: string;
  duration: number;
}

let _id = 0;
const toasts = ref<Toast[]>([]);

export function useToast() {
  function addToast(
    title: string,
    type: ToastType = "info",
    message?: string,
    duration = 4000,
  ) {
    const id = ++_id;
    toasts.value.push({ id, type, title, message, duration });
    setTimeout(() => removeToast(id), duration + 300); // 300ms for leave transition
  }

  function removeToast(id: number) {
    const idx = toasts.value.findIndex((t) => t.id === id);
    if (idx !== -1) toasts.value.splice(idx, 1);
  }

  return { toasts, addToast, removeToast };
}
