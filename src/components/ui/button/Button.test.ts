import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import Button from "./Button.vue";

describe("Button", () => {
  it("renders default slot text", () => {
    const wrapper = mount(Button, {
      slots: { default: "Click me" },
    });
    expect(wrapper.text()).toBe("Click me");
    expect(wrapper.find("button").exists()).toBe(true);
  });

  it("applies variant and size data attributes", () => {
    const wrapper = mount(Button, {
      props: { variant: "destructive", size: "sm" },
      slots: { default: "Delete" },
    });
    expect(wrapper.attributes("data-variant")).toBe("destructive");
    expect(wrapper.attributes("data-size")).toBe("sm");
  });
});
