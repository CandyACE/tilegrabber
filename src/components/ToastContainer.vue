<script setup lang="ts">
import { TransitionGroup } from "vue";
import { CheckCircle, AlertTriangle, XCircle, Info, X } from "lucide-vue-next";
import { useToast, type Toast } from "~/composables/useToast";

const { toasts, removeToast } = useToast();

const icons = {
  success: CheckCircle,
  warning: AlertTriangle,
  error: XCircle,
  info: Info,
};

const colorMap = {
  success: "bg-green-50 border-green-200 text-green-800",
  warning: "bg-amber-50 border-amber-200 text-amber-800",
  error: "bg-red-50 border-red-200 text-red-800",
  info: "bg-blue-50 border-blue-200 text-blue-800",
};

const iconColorMap = {
  success: "text-green-500",
  warning: "text-amber-500",
  error: "text-red-500",
  info: "text-blue-500",
};
</script>

<template>
  <Teleport to="body">
    <div
      class="fixed bottom-4 right-4 z-[9999] flex flex-col gap-2 pointer-events-none"
      style="max-width: 320px"
    >
      <TransitionGroup
        name="toast"
        tag="div"
        class="flex flex-col gap-2"
      >
        <div
          v-for="toast in toasts"
          :key="toast.id"
          :class="[
            'pointer-events-auto flex items-start gap-2.5 px-3.5 py-3 rounded-xl border shadow-lg text-xs',
            colorMap[toast.type],
          ]"
        >
          <component
            :is="icons[toast.type]"
            :size="15"
            :class="['shrink-0 mt-0.5', iconColorMap[toast.type]]"
          />
          <div class="flex-1 min-w-0">
            <div class="font-semibold leading-snug truncate">{{ toast.title }}</div>
            <div v-if="toast.message" class="mt-0.5 text-[11px] opacity-80 truncate">{{ toast.message }}</div>
          </div>
          <button
            @click="removeToast(toast.id)"
            class="shrink-0 opacity-50 hover:opacity-80 transition-opacity"
          >
            <X :size="13" />
          </button>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-enter-active {
  transition: all 0.25s ease;
}
.toast-leave-active {
  transition: all 0.2s ease;
}
.toast-enter-from {
  opacity: 0;
  transform: translateY(12px) scale(0.96);
}
.toast-leave-to {
  opacity: 0;
  transform: translateX(20px) scale(0.96);
}
</style>
