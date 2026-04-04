<script setup lang="ts">
import { ref } from "vue";

defineProps<{ open: boolean }>();
const emit = defineEmits<{
  agree: [];
  disagree: [];
}>();

// 点击按钮后触发退出动画，再 emit 事件
const leaving = ref(false);

function handleAgree() {
  leaving.value = true;
  setTimeout(() => emit("agree"), 600);
}

function handleDisagree() {
  leaving.value = true;
  setTimeout(() => emit("disagree"), 600);
}
</script>

<template>
  <Transition name="disclaimer-screen">
    <div v-if="open" class="disclaimer-root" :class="{ leaving }">
      <!-- 电影黑条 -->
      <div class="letterbox letterbox-top" />
      <div class="letterbox letterbox-bottom" />

      <!-- 中央内容 -->
      <div class="disclaimer-body">
        <!-- 顶部标签 -->
        <div class="tag">使用前须知</div>

        <!-- 主标题 -->
        <h1 class="title">用户须知与免责声明</h1>

        <!-- 分割线 -->
        <div class="divider" />

        <!-- 条款文本 -->
        <div class="clauses">
          <p class="intro">
            本软件（<em>御图</em>）仅供个人学习、科研及合法的离线地图使用场景。<br />
            继续使用前，请仔细阅读以下全部条款。
          </p>

          <ul class="clause-list">
            <li>
              <span class="clause-no">01</span>
              <div>
                <strong>版权合规</strong>
                —
                地图数据版权归各数据提供方所有。请确保您的使用符合相关服务条款，不得用于商业分发或违反数据提供方的授权协议。
              </div>
            </li>
            <li>
              <span class="clause-no">02</span>
              <div>
                <strong>合法使用</strong>
                —
                严禁用于任何违反国家法律法规或侵犯他人权益的行为。涉密地图数据受相关保密法律约束，请勿违规下载或存储。
              </div>
            </li>
            <li>
              <span class="clause-no">03</span>
              <div>
                <strong>风险自担</strong>
                —
                因使用本软件所产生的任何法律责任或损失，由使用者自行承担，软件开发者不承担任何连带责任。
              </div>
            </li>
            <li>
              <span class="clause-no">04</span>
              <div>
                <strong>封禁风险</strong>
                — 频繁、大量的瓦片请求可能触发数据提供方的反爬机制，导致您的 IP
                或账号被限流或封禁，请谨慎使用。
              </div>
            </li>
          </ul>
        </div>

        <!-- 操作按钮 -->
        <div class="actions">
          <button class="btn-disagree" @click="handleDisagree">
            不同意，退出
          </button>
          <button class="btn-agree" @click="handleAgree">
            我已阅读并同意 →
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
/* ── 全屏容器 ──────────────────────────────────────────────────────────────── */
.disclaimer-root {
  position: fixed;
  inset: 0;
  z-index: 9998;
  background: #080c12;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.disclaimer-root.leaving {
  animation: screen-fade-out 0.6s ease-in both;
}

@keyframes screen-fade-out {
  to {
    opacity: 0;
  }
}

/* Vue Transition（初始进场） */
.disclaimer-screen-enter-active {
  animation: screen-fade-in 0.5s ease-out both;
}
.disclaimer-screen-leave-active {
  /* 退场由 .leaving class 控制，这里不额外处理 */
  animation: none;
}
@keyframes screen-fade-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

/* ── 电影黑条 ──────────────────────────────────────────────────────────────── */
.letterbox {
  position: absolute;
  left: 0;
  right: 0;
  height: 14%;
  background: #000;
  z-index: 10;
}
.letterbox-top {
  top: 0;
  transform: translateY(-100%);
  animation: lb-top-in 0.55s cubic-bezier(0.22, 1, 0.36, 1) 0s both;
}
.letterbox-bottom {
  bottom: 0;
  transform: translateY(100%);
  animation: lb-bottom-in 0.55s cubic-bezier(0.22, 1, 0.36, 1) 0s both;
}
@keyframes lb-top-in {
  to {
    transform: translateY(0);
  }
}
@keyframes lb-bottom-in {
  to {
    transform: translateY(0);
  }
}

/* ── 主体 ──────────────────────────────────────────────────────────────────── */
.disclaimer-body {
  position: relative;
  z-index: 5;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  max-width: 620px;
  width: calc(100% - 80px);
  gap: 0;
}

/* ── 顶部标签 ─────────────────────────────────────────────────────────────── */
.tag {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: #0969da;
  margin-bottom: 18px;
  opacity: 0;
  animation: item-enter 0.6s cubic-bezier(0.16, 1, 0.3, 1) 0.4s both;
}

/* ── 标题 ─────────────────────────────────────────────────────────────────── */
.title {
  font-family: "SF Pro Display", "Helvetica Neue", system-ui, sans-serif;
  font-size: 28px;
  font-weight: 700;
  color: #e8edf2;
  line-height: 1.2;
  margin: 0 0 20px 0;
  opacity: 0;
  animation: item-enter 0.7s cubic-bezier(0.16, 1, 0.3, 1) 0.55s both;
}

/* ── 分割线 ───────────────────────────────────────────────────────────────── */
.divider {
  width: 0;
  height: 1px;
  background: linear-gradient(90deg, #0969da 0%, #58a6ff 50%, transparent 100%);
  margin-bottom: 28px;
  opacity: 0;
  animation:
    divider-grow 0.7s cubic-bezier(0.16, 1, 0.3, 1) 0.75s both,
    divider-show 0s 0.75s both;
}
@keyframes divider-show {
  to {
    opacity: 1;
  }
}
@keyframes divider-grow {
  to {
    width: 260px;
  }
}

/* ── 条款区域 ─────────────────────────────────────────────────────────────── */
.clauses {
  display: flex;
  flex-direction: column;
  gap: 0;
  width: 100%;
  opacity: 0;
  animation: item-enter 0.7s cubic-bezier(0.16, 1, 0.3, 1) 0.9s both;
}

.intro {
  font-size: 13px;
  line-height: 1.7;
  color: #8b9eb0;
  margin-bottom: 20px;
}

.intro em {
  font-style: normal;
  color: #b0c4d8;
  font-weight: 500;
}

.clause-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  margin-bottom: 32px;
}

.clause-list li {
  display: flex;
  align-items: flex-start;
  gap: 14px;
  font-size: 12.5px;
  line-height: 1.65;
  color: #64788a;
}

.clause-no {
  flex-shrink: 0;
  font-family: "IBM Plex Mono", "Cascadia Code", monospace;
  font-size: 11px;
  font-weight: 600;
  color: #1e3a5f;
  margin-top: 2px;
  letter-spacing: 0.05em;
}

.clause-list strong {
  color: #8fafc6;
  font-weight: 600;
}

/* ── 操作按钮 ─────────────────────────────────────────────────────────────── */
.actions {
  display: flex;
  align-items: center;
  gap: 14px;
  opacity: 0;
  animation: item-enter 0.6s cubic-bezier(0.16, 1, 0.3, 1) 1.2s both;
}

.btn-disagree {
  padding: 9px 18px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 500;
  color: #455566;
  background: transparent;
  border: 1px solid #1e2d3d;
  cursor: pointer;
  transition:
    color 0.15s,
    border-color 0.15s;
}
.btn-disagree:hover {
  color: #7a8fa0;
  border-color: #2d4158;
}

.btn-agree {
  padding: 9px 22px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
  color: #fff;
  background: #0969da;
  border: none;
  cursor: pointer;
  transition:
    background 0.15s,
    opacity 0.15s;
  letter-spacing: 0.01em;
}
.btn-agree:hover {
  background: #0757bb;
}
.btn-agree:active {
  opacity: 0.85;
}

/* ── 共用入场动画 ─────────────────────────────────────────────────────────── */
@keyframes item-enter {
  from {
    opacity: 0;
    transform: translateY(12px);
    filter: blur(2px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
    filter: blur(0);
  }
}
</style>
