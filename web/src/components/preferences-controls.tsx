/**
 * [INPUT]: 依赖语言上下文、主题状态、Select 和共享弹窗。
 * [OUTPUT]: 为登录页和个人设置提供语言、外观选择。
 * [POS]: 两处入口复用同一组偏好字段；动画不提供用户开关。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useState } from "react";
import { useTheme } from "next-themes";
import {
  Globe2,
  Monitor,
  Moon,
  SlidersHorizontal,
  Sun,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Modal } from "@/components/ui/dialog";
import { usePreferences, useUI } from "@/components/providers";
import { isLocale } from "@/lib/preferences";

export function PreferenceFields({ expanded = false }: { expanded?: boolean }) {
  const { t } = useUI();
  const { locale, setLocale, ready } = usePreferences();
  const { theme, resolvedTheme, setTheme } = useTheme();
  const ThemeIcon =
    !ready || theme === "system"
      ? Monitor
      : resolvedTheme === "dark"
        ? Moon
        : Sun;
  return (
    <div
      className={expanded ? "preference-fields expanded" : "preference-fields"}
    >
      <label className="preference-select">
        <Globe2 size={15} />
        <span className="sr-only">{t("preferences.language")}</span>
        <Select
          aria-label={t("preferences.language")}
          value={locale}
          onChange={(event) => {
            if (isLocale(event.target.value)) setLocale(event.target.value);
          }}
        >
          <option value="zh-CN">简体中文</option>
          <option value="en">English</option>
        </Select>
      </label>
      <label className="preference-select">
        <ThemeIcon size={15} />
        <span className="sr-only">{t("preferences.theme")}</span>
        <Select
          aria-label={t("preferences.theme")}
          value={ready ? theme : "system"}
          onChange={(event) => setTheme(event.target.value)}
        >
          <option value="system">{t("preferences.system")}</option>
          <option value="dark">{t("preferences.dark")}</option>
          <option value="light">{t("preferences.light")}</option>
        </Select>
      </label>
    </div>
  );
}

export function PreferencesControls() {
  const { t } = useUI();
  const [open, setOpen] = useState(false);
  return (
    <div className="preferences-controls">
      <div className="preferences-desktop">
        <PreferenceFields />
      </div>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="preferences-mobile"
        aria-label={t("preferences.title")}
        onClick={() => setOpen(true)}
      >
        <SlidersHorizontal />
      </Button>
      <Modal
        open={open}
        onOpenChange={setOpen}
        title={t("preferences.title")}
        description={t("preferences.description")}
      >
        <PreferenceFields expanded />
      </Modal>
    </div>
  );
}
