"use client";
/**
 * [INPUT]: 依赖租户会话、签名公开 API、通知和统一表单。
 * [OUTPUT]: 提供凭据导入、版本停用、方案和项目默认方案。
 * [POS]: 租户与项目共用界面；方案加载后初始化默认选择。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { useState } from "react";
import { useRemote, mutation } from "@/lib/api";
import { useUI } from "@/components/providers";
import { Field, ErrorBox, Loading } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import type { Project, SigningBinding, SigningMetadata } from "@/lib/types";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";

type Version = { id: string; metadata: SigningMetadata; enabled: boolean };
type Credential = {
  id: string;
  name: string;
  platform: string;
  enabled: boolean;
  versions: Version[];
};
type Profile = SigningBinding & {
  enabled: boolean;
  credential_enabled: boolean;
  available: boolean;
};

export function SigningSummary({ bindings }: { bindings: SigningBinding[] }) {
  const { t, locale } = useUI();
  if (!bindings.length)
    return <p className="management-description">{t("signing.noDefaults")}</p>;
  return (
    <ul className="space-y-3">
      {bindings.map((binding) => (
        <li key={binding.platform}>
          <strong>
            {binding.platform} · {binding.profile}
          </strong>
          <p className="text-sm text-muted-foreground">
            {binding.metadata.identity} ·{" "}
            {new Date(binding.metadata.not_after * 1000).toLocaleDateString(
              locale,
            )}
          </p>
          <p className="break-all font-mono text-xs">
            {t("signing.version")}: {binding.version_id}
          </p>
          <p className="break-all font-mono text-xs">
            SHA-256: {binding.metadata.fingerprint_sha256}
          </p>
          {binding.notarize && (
            <span className="text-sm">{t("signing.notarizeRequired")}</span>
          )}
          {binding.available === false && (
            <p role="status" className="text-sm text-destructive">
              {t("signing.unavailable")}
            </p>
          )}
        </li>
      ))}
    </ul>
  );
}

async function file64(file: FormDataEntryValue | null) {
  if (!(file instanceof File) || !file.size || file.size > 2 * 1024 * 1024)
    throw new Error("Signing file must contain 1 byte to 2 MiB");
  const bytes = new Uint8Array(await file.arrayBuffer());
  let text = "";
  for (let index = 0; index < bytes.length; index += 8192)
    text += String.fromCharCode(...bytes.subarray(index, index + 8192));
  return btoa(text);
}

export function SigningSettings() {
  const { t, locale } = useUI();
  const credentials = useRemote<{
    configured: boolean;
    credentials: Credential[];
  }>("/signing/credentials");
  const profiles = useRemote<Profile[]>("/signing/profiles");
  const [rotate, setRotate] = useState("");
  const [platform, setPlatform] = useState("android");
  const [notary, setNotary] = useState(false);
  const [generation, setGeneration] = useState(0);
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const selected = credentials.data?.credentials.find(
    (value) => value.id === rotate,
  );
  const currentPlatform = selected?.platform || platform;
  async function toggle(resource: string, id: string, enabled: boolean) {
    setBusy(true);
    setError(undefined);
    try {
      await mutation(
        `/signing/${resource}/${encodeURIComponent(id)}`,
        { enabled },
        "PATCH",
      );
      notify.success(t("common.operationSucceeded"));
      await Promise.all([credentials.reload(), profiles.reload()]);
    } catch (error) {
      setError(asError(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="space-y-6">
      <ErrorBox message={error || credentials.error || profiles.error} />
      <section className="panel management-section">
        <h2>{t("signing.title")}</h2>
        <p className="management-description">{t("signing.description")}</p>
        {credentials.data && !credentials.data.configured && (
          <p role="status" className="notice">
            {t("signing.notConfigured")}
          </p>
        )}
        <fieldset disabled={!credentials.data?.configured}>
          <SubmitForm
            key={generation}
            onSubmit={async (form) => {
              const material =
                currentPlatform === "macos"
                  ? {
                      kind: "macos_pkcs12",
                      file_base64: await file64(form.get("file")),
                      password: form.get("password"),
                      ...(notary
                        ? {
                            notary: {
                              key_id: form.get("key_id"),
                              issuer_id: form.get("issuer_id"),
                              private_key: await (
                                form.get("notary_file") as File
                              ).text(),
                            },
                          }
                        : {}),
                    }
                  : {
                      kind: "android_keystore",
                      file_base64: await file64(form.get("file")),
                      store_password: form.get("password"),
                      key_password: form.get("key_password"),
                      alias: form.get("alias"),
                      purpose: form.get("purpose"),
                    };
              await mutation(
                rotate
                  ? `/signing/credentials/${rotate}/versions`
                  : "/signing/credentials",
                { ...(rotate ? {} : { name: form.get("name") }), material },
              );
              setGeneration((value) => value + 1);
              setNotary(false);
              await Promise.all([credentials.reload(), profiles.reload()]);
            }}
          >
            <Field label={t("signing.importMode")}>
              <select
                value={rotate}
                onChange={(event) => {
                  setRotate(event.target.value);
                  setNotary(false);
                }}
              >
                <option value="">{t("signing.newCredential")}</option>
                {credentials.data?.credentials.map((value) => (
                  <option key={value.id} value={value.id}>
                    {t("signing.rotate")}: {value.name}
                  </option>
                ))}
              </select>
            </Field>
            {!rotate && (
              <>
                <Field label={t("signing.name")}>
                  <Input name="name" required maxLength={100} />
                </Field>
                <Field label={t("signing.platform")}>
                  <select
                    value={platform}
                    onChange={(event) => {
                      setPlatform(event.target.value);
                      setNotary(false);
                    }}
                  >
                    <option value="android">Android</option>
                    <option value="macos">macOS</option>
                  </select>
                </Field>
              </>
            )}
            <Field label={t("signing.file")} hint={t("signing.fileHint")}>
              <Input
                type="file"
                name="file"
                accept=".jks,.keystore,.p12,.pfx"
                required
              />
            </Field>
            <Field label={t("signing.password")}>
              <Input
                type="password"
                name="password"
                autoComplete="new-password"
              />
            </Field>
            {currentPlatform === "android" ? (
              <>
                <Field label={t("signing.alias")}>
                  <Input name="alias" required autoComplete="off" />
                </Field>
                <Field label={t("signing.keyPassword")}>
                  <Input
                    name="key_password"
                    type="password"
                    autoComplete="new-password"
                  />
                </Field>
                <Field
                  label={t("signing.purpose")}
                  hint={t("signing.identityHint")}
                >
                  <select name="purpose">
                    <option value="application">
                      {t("signing.applicationKey")}
                    </option>
                    <option value="upload">{t("signing.uploadKey")}</option>
                  </select>
                </Field>
              </>
            ) : (
              <>
                <p className="management-description">
                  {t("signing.appleHint")}
                </p>
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={notary}
                    onChange={(event) => setNotary(event.target.checked)}
                  />
                  {t("signing.notaryCredentials")}
                </label>
                {notary && (
                  <>
                    <Field label="App Store Connect Key ID">
                      <Input name="key_id" required maxLength={10} />
                    </Field>
                    <Field label="Issuer ID">
                      <Input name="issuer_id" required />
                    </Field>
                    <Field label={t("signing.notaryFile")}>
                      <Input
                        type="file"
                        name="notary_file"
                        accept=".p8"
                        required
                      />
                    </Field>
                  </>
                )}
              </>
            )}
          </SubmitForm>
        </fieldset>
      </section>
      <section className="panel management-section">
        <h2>{t("signing.credentials")}</h2>
        {!credentials.data ? (
          !credentials.error && <Loading />
        ) : (
          credentials.data.credentials.map((credential) => (
            <div className="border-b py-4" key={credential.id}>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <strong>
                  {credential.name} · {credential.platform}
                </strong>
                <Button
                  disabled={busy}
                  variant="secondary"
                  onClick={() =>
                    void toggle(
                      "credentials",
                      credential.id,
                      !credential.enabled,
                    )
                  }
                >
                  {t(credential.enabled ? "signing.disable" : "signing.enable")}
                </Button>
              </div>
              {credential.versions.map((version, index) => (
                <div className="mt-3 space-y-1" key={version.id}>
                  <p className="text-sm">
                    {index === 0 ? t("signing.latest") : t("signing.version")} ·{" "}
                    {version.metadata.identity} · {t("signing.expires")}{" "}
                    {new Date(
                      version.metadata.not_after * 1000,
                    ).toLocaleDateString(locale)}
                  </p>
                  <p className="break-all font-mono text-xs">
                    {version.id} · {version.metadata.fingerprint_sha256}
                  </p>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() =>
                      void toggle("versions", version.id, !version.enabled)
                    }
                  >
                    {t(
                      version.enabled
                        ? "signing.disableVersion"
                        : "signing.enableVersion",
                    )}
                  </Button>
                </div>
              ))}
            </div>
          ))
        )}
      </section>
      <section className="panel management-section">
        <h2>{t("signing.profiles")}</h2>
        <p className="management-description">{t("signing.nodeHint")}</p>
        <fieldset
          disabled={
            !credentials.data?.configured ||
            !credentials.data.credentials.length
          }
        >
          <SubmitForm
            onSubmit={async (form) => {
              await mutation("/signing/profiles", {
                name: form.get("name"),
                credential_id: form.get("credential"),
                notarize: form.get("notarize") === "on",
              });
              await profiles.reload();
            }}
          >
            <Field label={t("signing.profileName")}>
              <Input
                name="name"
                pattern="[A-Za-z0-9._-]+"
                required
                maxLength={80}
              />
            </Field>
            <Field label={t("signing.credentials")}>
              <select name="credential" required>
                <option value="">{t("signing.select")}</option>
                {credentials.data?.credentials
                  .filter((value) => value.enabled)
                  .map((value) => (
                    <option key={value.id} value={value.id}>
                      {value.name} · {value.platform}
                    </option>
                  ))}
              </select>
            </Field>
            <label className="flex items-center gap-2">
              <input type="checkbox" name="notarize" />
              {t("signing.notarizeRequired")}
            </label>
          </SubmitForm>
        </fieldset>
        {profiles.data?.map((profile) => (
          <div
            className="flex flex-wrap items-center justify-between gap-3 border-b py-3"
            key={profile.profile}
          >
            <span>
              {profile.profile} · {profile.platform}
              {profile.notarize ? ` · ${t("signing.notarizeRequired")}` : ""}
            </span>
            <Button
              disabled={busy}
              variant="secondary"
              onClick={() =>
                void toggle("profiles", profile.profile, !profile.enabled)
              }
            >
              {t(profile.enabled ? "signing.disable" : "signing.enable")}
            </Button>
          </div>
        ))}
      </section>
    </div>
  );
}

export function ProjectSigning({
  project,
  editable,
  reload,
}: {
  project: Project;
  editable: boolean;
  reload: () => void;
}) {
  const { t } = useUI();
  const profiles = useRemote<Profile[]>(editable ? "/signing/profiles" : null);
  const preview = useRemote<SigningBinding[]>(
    `/projects/${project.id}/signing-preview`,
  );
  return (
    <section className="panel management-section">
      <h2>{t("signing.defaults")}</h2>
      <p className="management-description">{t("signing.defaultsHint")}</p>
      <ErrorBox message={profiles.error || preview.error} />
      <SigningSummary bindings={preview.data || []} />
      {editable &&
        (!profiles.data ? (
          !profiles.error && <Loading variant="form" />
        ) : (
          <SubmitForm
            key={JSON.stringify(project.signing_defaults)}
            onSubmit={async (form) => {
              const defaults = Object.fromEntries(
                ["macos", "android"]
                  .map((platform) => [platform, form.get(platform)])
                  .filter(([, value]) => value),
              );
              await mutation(
                `/projects/${project.id}/signing-defaults`,
                defaults,
                "PUT",
              );
              await preview.reload();
              reload();
            }}
          >
            {["macos", "android"].map((platform) => (
              <Field key={platform} label={platform}>
                <select
                  name={platform}
                  defaultValue={project.signing_defaults?.[platform] || ""}
                >
                  <option value="">{t("signing.noDefault")}</option>
                  {profiles.data
                    ?.filter((profile) => profile.platform === platform)
                    .map((profile) => {
                      const name = profile.profile;
                      return (
                        <option
                          key={name}
                          value={name}
                          disabled={
                            !profile.available &&
                            name !== project.signing_defaults?.[platform]
                          }
                        >
                          {name}
                        </option>
                      );
                    })}
                </select>
              </Field>
            ))}
          </SubmitForm>
        ))}
    </section>
  );
}
