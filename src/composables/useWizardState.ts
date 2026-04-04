import { ref } from 'vue'

/**
 * 全局向导状态 — 让布局层和页面层共享「是否显示新建任务向导」的状态
 */
const showWizard = ref(false)

export function useWizardState() {
  function openWizard() {
    showWizard.value = true
  }

  function closeWizard() {
    showWizard.value = false
  }

  return { showWizard, openWizard, closeWizard }
}
