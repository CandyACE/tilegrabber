<script setup lang="ts">
import { nextTick, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Pencil, Check, X } from "lucide-vue-next";
import { useI18n } from "vue-i18n";

const props = defineProps<{ taskId: string; name: string }>();
const emit = defineEmits<{ saved: [name: string] }>();
const { t } = useI18n();
const editing = ref(false);
const draft = ref("");
const saving = ref(false);
const error = ref("");
const input = ref<HTMLInputElement>();

// 编辑入口阻止卡片点击冒泡，避免重命名时打开任务详情。
async function begin() {
  draft.value = props.name;
  error.value = "";
  editing.value = true;
  await nextTick();
  input.value?.focus();
  input.value?.select();
}

async function save() {
  if (saving.value) return;
  const name = draft.value.trim();
  if (!name || [...name].length > 200) {
    error.value = t("taskRename.invalid");
    return;
  }
  saving.value = true;
  error.value = "";
  try {
    const saved = await invoke<string>("rename_task", { taskId: props.taskId, name });
    console.info("[TaskNameEditor] 任务重命名成功", { taskId: props.taskId });
    emit("saved", saved);
    editing.value = false;
  } catch (e) {
    console.error("[TaskNameEditor] 任务重命名失败", e);
    error.value = t("taskRename.failed", { error: String(e) });
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="min-w-0 flex-1" @click.stop @keydown.stop>
    <form v-if="editing" class="flex items-center gap-1" @submit.prevent="save">
      <input ref="input" v-model="draft" :aria-label="t('taskRename.label')"
        :disabled="saving" class="min-w-0 w-full rounded border px-1 py-0.5 text-sm"
        @keydown.esc.prevent="!saving && (editing = false)" />
      <button type="submit" :disabled="saving" :title="t('common.confirm')"><Check class="size-4" /></button>
      <button type="button" :disabled="saving" :title="t('common.cancel')" @click="editing = false"><X class="size-4" /></button>
    </form>
    <div v-else class="flex items-start gap-1">
      <span class="truncate text-sm font-medium" :title="name">{{ name }}</span>
      <button type="button" class="shrink-0 p-0.5 text-slate-400 hover:text-blue-600" :title="t('taskRename.label')" @click="begin"><Pencil class="size-3" /></button>
    </div>
    <p v-if="editing && error" role="alert" class="text-xs text-red-500">{{ error }}</p>
  </div>
</template>
