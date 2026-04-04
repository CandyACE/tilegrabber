<script setup lang="ts">
import { onMounted } from "vue";
import logoUrl from "~/assets/logo.png";

const emit = defineEmits<{ done: [] }>();

onMounted(() => {
  // 总时长：3.8s 后通知父级隐藏
  setTimeout(() => emit("done"), 3800);
});
</script>

<template>
  <div class="splash-root">
    <!-- 电影遮幕：上下黑条（开场展开→收场合拢） -->
    <div class="letterbox letterbox-top" />
    <div class="letterbox letterbox-bottom" />

    <!-- 中央内容 -->
    <div class="splash-center">
      <!-- Logo 图标 -->
      <div class="logo-wrap">
        <img :src="logoUrl" class="logo-icon" alt="御图" />
        <!-- 光晕扫过效果 -->
        <div class="logo-shine" />
      </div>

      <!-- 应用名称 -->
      <div class="app-name">御图</div>

      <!-- 副标题 -->
      <div class="app-sub">地理位置瓦片下载与管理工具</div>

      <!-- 底部装饰线 -->
      <div class="accent-line" />
    </div>
  </div>
</template>

<style scoped>
/* ── 根容器 ─────────────────────────────────────────────────────────────── */
.splash-root {
  position: fixed;
  inset: 0;
  z-index: 9999;
  background: #080c12;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  /* 整体在 3.2s 开始淡出 */
  animation: splash-fade-out 0.6s ease-in 3.2s both;
}

@keyframes splash-fade-out {
  to {
    opacity: 0;
  }
}

/* ── 电影黑条 ────────────────────────────────────────────────────────────── */
.letterbox {
  position: absolute;
  left: 0;
  right: 0;
  height: 18%;
  background: #000;
  z-index: 10;
  will-change: transform;
}

.letterbox-top {
  top: 0;
  transform: translateY(-100%);
  animation:
    lb-top-in 0.55s cubic-bezier(0.22, 1, 0.36, 1) 0s both,
    lb-top-out 0.45s cubic-bezier(0.65, 0, 0.35, 1) 3.1s both;
}

.letterbox-bottom {
  bottom: 0;
  transform: translateY(100%);
  animation:
    lb-bottom-in 0.55s cubic-bezier(0.22, 1, 0.36, 1) 0s both,
    lb-bottom-out 0.45s cubic-bezier(0.65, 0, 0.35, 1) 3.1s both;
}

@keyframes lb-top-in {
  to {
    transform: translateY(0);
  }
}
@keyframes lb-top-out {
  to {
    transform: translateY(-100%);
  }
}
@keyframes lb-bottom-in {
  to {
    transform: translateY(0);
  }
}
@keyframes lb-bottom-out {
  to {
    transform: translateY(100%);
  }
}

/* ── 中央内容区 ──────────────────────────────────────────────────────────── */
.splash-center {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0;
  position: relative;
  z-index: 5;
}

/* ── Logo ────────────────────────────────────────────────────────────────── */
.logo-wrap {
  position: relative;
  width: 80px;
  height: 80px;
  margin-bottom: 28px;
  animation: logo-enter 0.8s cubic-bezier(0.16, 1, 0.3, 1) 0.55s both;
}

.logo-icon {
  width: 80px;
  height: 80px;
  border-radius: 20px;
  object-fit: contain;
  filter: drop-shadow(0 8px 32px rgba(9, 105, 218, 0.45))
    drop-shadow(0 32px 60px rgba(9, 105, 218, 0.15));
}

/* 扫光 */
.logo-shine {
  position: absolute;
  inset: 0;
  border-radius: 20px;
  overflow: hidden;
  pointer-events: none;
  animation: shine-delay 0.55s both;
}

.logo-shine::after {
  content: "";
  position: absolute;
  top: -50%;
  left: -80%;
  width: 60%;
  height: 200%;
  background: linear-gradient(
    105deg,
    transparent 30%,
    rgba(255, 255, 255, 0.18) 50%,
    transparent 70%
  );
  animation: shine-sweep 0.9s ease-out 1s both;
}

@keyframes shine-delay {
  from,
  to {
    opacity: 1;
  }
}

@keyframes shine-sweep {
  from {
    transform: translateX(0);
  }
  to {
    transform: translateX(380%);
  }
}

@keyframes logo-enter {
  from {
    opacity: 0;
    transform: scale(0.78) translateY(12px);
    filter: blur(4px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
    filter: blur(0);
  }
}

/* ── 应用名 ───────────────────────────────────────────────────────────────── */
.app-name {
  font-family: "SF Pro Display", "Helvetica Neue", system-ui, sans-serif;
  font-size: 38px;
  font-weight: 700;
  letter-spacing: -0.5px;
  color: #f0f4f8;
  line-height: 1;
  margin-bottom: 10px;
  animation: text-enter 0.7s cubic-bezier(0.16, 1, 0.3, 1) 0.9s both;
}

/* ── 副标题 ───────────────────────────────────────────────────────────────── */
.app-sub {
  font-family: system-ui, sans-serif;
  font-size: 13px;
  font-weight: 400;
  letter-spacing: 0.08em;
  color: #57687a;
  text-transform: none;
  margin-bottom: 28px;
  animation: text-enter 0.6s cubic-bezier(0.16, 1, 0.3, 1) 1.2s both;
}

/* ── 装饰横线 ────────────────────────────────────────────────────────────── */
.accent-line {
  width: 0;
  height: 2px;
  background: linear-gradient(
    90deg,
    transparent,
    #0969da 40%,
    #58a6ff 60%,
    transparent
  );
  border-radius: 2px;
  animation: line-grow 0.7s cubic-bezier(0.16, 1, 0.3, 1) 1.45s both;
}

@keyframes line-grow {
  to {
    width: 120px;
  }
}

@keyframes text-enter {
  from {
    opacity: 0;
    transform: translateY(14px);
    filter: blur(3px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
    filter: blur(0);
  }
}
</style>
