<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

defineProps<{ open: boolean }>();
const emit = defineEmits<{
  confirm: [action: "quit" | "tray", remember: boolean];
  cancel: [];
}>();

const selected = ref<"tray" | "quit">("tray");
const remember = ref(false);
</script>

<template>
  <Transition name="modal-fade">
    <div
      v-if="open"
      class="fixed inset-0 z-9999 flex items-center justify-center p-4"
      style="background: rgba(0, 0, 0, 0.45)"
    >
      <div
        class="relative w-full max-w-sm rounded-2xl shadow-2xl flex flex-col overflow-hidden"
        style="background: var(--color-surface, #fff)"
      >
        <!-- 头部 -->
        <div class="px-6 pt-5 pb-3">
          <p
            class="text-sm font-semibold"
            style="color: var(--color-text-primary, #0f172a)"
          >
            {{ t('closeDialog.title') }}
          </p>
          <p
            class="text-xs mt-1"
            style="color: var(--color-text-muted, #64748b)"
          >
            {{ t('closeDialog.subtitle') }}
          </p>
        </div>

        <!-- 选项 -->
        <div class="px-6 pb-4 flex flex-col gap-2">
          <label
            class="flex items-start gap-3 p-3 rounded-xl cursor-pointer border transition-colors"
            :style="
              selected === 'tray'
                ? 'border-color: var(--color-accent, #3b82f6); background: #eff6ff'
                : 'border-color: var(--color-border-subtle, #e2e8f0); background: transparent'
            "
          >
            <input
              v-model="selected"
              type="radio"
              value="tray"
              class="mt-0.5 accent-blue-500"
            />
            <div>
              <p
                class="text-sm font-medium"
                style="color: var(--color-text-primary, #0f172a)"
              >
                {{ t('closeDialog.optTrayLabel') }}
              </p>
              <p
                class="text-xs mt-0.5"
                style="color: var(--color-text-muted, #64748b)"
              >
                {{ t('closeDialog.optTrayDesc') }}
              </p>
            </div>
          </label>

          <label
            class="flex items-start gap-3 p-3 rounded-xl cursor-pointer border transition-colors"
            :style="
              selected === 'quit'
                ? 'border-color: var(--color-accent, #3b82f6); background: #eff6ff'
                : 'border-color: var(--color-border-subtle, #e2e8f0); background: transparent'
            "
          >
            <input
              v-model="selected"
              type="radio"
              value="quit"
              class="mt-0.5 accent-blue-500"
            />
            <div>
              <p
                class="text-sm font-medium"
                style="color: var(--color-text-primary, #0f172a)"
              >
                {{ t('closeDialog.optQuitLabel') }}
              </p>
              <p
                class="text-xs mt-0.5"
                style="color: var(--color-text-muted, #64748b)"
              >
                {{ t('closeDialog.optQuitDesc') }}
              </p>
            </div>
          </label>

          <!-- 记忆选项 -->
          <label
            class="flex items-center gap-2 mt-1 cursor-pointer select-none"
          >
            <input v-model="remember" type="checkbox" class="accent-blue-500" />
            <span
              class="text-xs"
              style="color: var(--color-text-secondary, #334155)"
            >
              {{ t('closeDialog.remember') }}
            </span>
          </label>
        </div>

        <!-- 操作 -->
        <div
          class="flex items-center justify-end gap-3 px-6 py-4 border-t"
          style="border-color: var(--color-border-subtle, #e2e8f0)"
        >
          <button
            class="px-4 py-2 rounded-lg text-sm font-medium transition-colors"
            style="color: var(--color-text-muted, #64748b)"
            @click="emit('cancel')"
          >
            {{ t('closeDialog.cancel') }}
          </button>
          <button
            class="px-5 py-2 rounded-lg text-sm font-semibold text-white transition-colors hover:opacity-90 active:opacity-80"
            style="background: var(--color-accent, #3b82f6)"
            @click="emit('confirm', selected, remember)"
          >
            {{ t('closeDialog.confirm') }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.modal-fade-enter-active,
.modal-fade-leave-active {
  transition: opacity 0.2s ease;
}
.modal-fade-enter-active .relative,
.modal-fade-leave-active .relative {
  transition:
    transform 0.2s ease,
    opacity 0.2s ease;
}
.modal-fade-enter-from,
.modal-fade-leave-to {
  opacity: 0;
}
.modal-fade-enter-from .relative,
.modal-fade-leave-to .relative {
  transform: scale(0.97) translateY(4px);
  opacity: 0;
}
</style>
