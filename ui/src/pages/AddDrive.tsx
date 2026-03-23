import { Alert, Box, Button, CircularProgress, Container, InputAdornment, Snackbar, Typography } from "@mui/material";
import { openUrl, openPath } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrent as getCurrentDeepLink, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { platform } from "@tauri-apps/plugin-os";
import { invoke } from '@tauri-apps/api/core';
import confetti from "canvas-confetti";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams } from "react-router-dom";
import defaultLogo from "../assets/cloudreve.svg";
import { FilledTextField } from "../common/StyledComponent";
import { useIsWindows10 } from "../hooks/useIsWindows10";
import { fetchSiteIcon, isValidUrl } from "../utils/manifest";
import { generatePKCEPair, randomCryptoString } from "../utils/pkce";
import {
  exchangeTokens,
  isValidationError,
  validateSiteVersion,
  type TokenResponse,
} from "../utils/siteValidation";
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { CALLBACK_PATH, CLIENT_ID, SCOPES } from "../utils/constants";

type PageState = "url_input" | "waiting" | "final_setup" | "setting_up" | "success";

interface OAuthCallbackData {
  code: string;
  state: string;
  name: string;
  path: string;
  user_id?: string;
}

// Store PKCE data for use after OAuth redirect
export interface PKCESession {
  codeVerifier: string;
  codeChallenge: string;
  siteUrl: string;
  siteVersion: string;
  siteIcon?: string;
  state?: string;
  callbackData?: OAuthCallbackData;
}

interface AddDriveProps {
  mode?: "add" | "reauthorize";
}

interface LocalPathValidation {
  normalized_path: string;
  warning?: string | null;
}

function parseDeeplinkUrl(url: string): OAuthCallbackData | null {
  try {
    // Handle custom protocol: cloudreve://callback/desktop?code=xxx&state=xxx
    const urlObj = new URL(url);
    const code = urlObj.searchParams.get("code");
    const state = urlObj.searchParams.get("state");
    const name = urlObj.searchParams.get("name") || "";
    const path = urlObj.searchParams.get("path") || "";
    const user_id = urlObj.searchParams.get("user_id") || "";
    if (code && state) {
      return { code, state, name, path, user_id };
    }
    return null;
  } catch {
    return null;
  }
}

function buildAuthorizeUrl(siteUrl: string, codeChallenge: string, state: string): string {
  const url = new URL("/session/authorize", siteUrl);
  const params = {
    response_type: "code",
    client_id: CLIENT_ID,
    scope: SCOPES,
    redirect_uri: CALLBACK_PATH,
    code_challenge: codeChallenge,
    code_challenge_method: "S256",
    state: state,
  };
  // Use encodeURIComponent to encode spaces as %20 instead of +
  url.search = Object.entries(params)
    .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`)
    .join("&");
  return url.toString();
}

export default function AddDrive({ mode = "add" }: AddDriveProps) {
  const { t } = useTranslation();
  const { driveId, siteUrl: encodedSiteUrl, driveName: driveNameQuery } = useParams<{ driveId?: string; siteUrl?: string, driveName: string }>();
  const isReauthorize = mode === "reauthorize" && driveId && encodedSiteUrl;
  const decodedSiteUrl = encodedSiteUrl ? decodeURIComponent(encodedSiteUrl) : "";
  const isWindows10 = useIsWindows10();
  const [osPlatform, setOsPlatform] = useState<string>("unknown");

  const [siteUrl, setSiteUrl] = useState(isReauthorize ? decodedSiteUrl : "");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [snackbarOpen, setSnackbarOpen] = useState(false);
  const [logo, setLogo] = useState(defaultLogo);
  const [authorizeUrl, setAuthorizeUrl] = useState<string | null>(null);
  const [pageState, setPageState] = useState<PageState>(isReauthorize ? "url_input" : "url_input");
  const [localPath, setLocalPath] = useState("");
  const [localPathError, setLocalPathError] = useState("");
  const [localPathWarning, setLocalPathWarning] = useState("");
  const [validatingLocalPath, setValidatingLocalPath] = useState(false);
  const [driveName, setDriveName] = useState(driveNameQuery ? decodeURIComponent(driveNameQuery) : "");
  const lastFetchedUrl = useRef<string>("");
  const currentIconUrl = useRef<string | undefined>(undefined);
  const pkceSessionRef = useRef<PKCESession | null>(null);
  const hasInitialized = useRef(false);

  useEffect(() => {
    try {
      setOsPlatform(platform());
    } catch {
      setOsPlatform("unknown");
    }
  }, []);

  const localPathPlaceholder = (() => {
    if (osPlatform === "macos") {
      return t("addDrive.localPathPlaceholderMac");
    }
    if (osPlatform === "windows") {
      return t("addDrive.localPathPlaceholderWin");
    }
    return t("addDrive.localPathPlaceholder");
  })();

  const localPathTips = (() => {
    if (osPlatform === "macos") {
      return t("addDrive.localPathTipsMac");
    }
    if (osPlatform === "windows") {
      return t("addDrive.localPathTipsWin");
    }
    return t("addDrive.localPathTipsOther");
  })();

  const handleOAuthCallbackUrl = useCallback((url: string) => {
    console.log("Received OAuth callback URL:", url);

    const callbackData = parseDeeplinkUrl(url);
    if (!callbackData) {
      console.error("Failed to parse deeplink URL:", url);
      return;
    }

    if (!pkceSessionRef.current || callbackData.state !== pkceSessionRef.current.state) {
      console.error("State mismatch or no active session", pkceSessionRef.current, callbackData);
      setError(t("addDrive.errors.stateMismatch"));
      setSnackbarOpen(true);
      return;
    }

    pkceSessionRef.current.callbackData = callbackData;
    callbackData.name && setDriveName(callbackData.name);
    setPageState("final_setup");
  }, [t]);

  // Listen for deeplink events from OAuth callback
  useEffect(() => {
    let unlistenLegacy: (() => void) | undefined;
    let unlistenPlugin: (() => void) | undefined;
    let disposed = false;

    listen<string>('deeplink', (event) => {
      handleOAuthCallbackUrl(event.payload);
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlistenLegacy = fn;
    });

    onOpenUrl((urls) => {
      urls.forEach((url) => handleOAuthCallbackUrl(url));
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlistenPlugin = fn;
    }).catch((error) => {
      console.warn("Failed to subscribe to plugin deep link events:", error);
    });

    getCurrentDeepLink()
      .then((urls) => {
        urls?.forEach((url) => handleOAuthCallbackUrl(url));
      })
      .catch((error) => {
        console.warn("Failed to get current deep link payload:", error);
      });

    return () => {
      disposed = true;
      unlistenLegacy?.();
      unlistenPlugin?.();
    };
  }, [handleOAuthCallbackUrl]);

  // Fetch site icon when URL changes and is valid
  const handleUrlBlur = () => {
    const trimmedUrl = siteUrl.trim();
    if (!isValidUrl(trimmedUrl) || trimmedUrl === lastFetchedUrl.current) {
      return;
    }

    lastFetchedUrl.current = trimmedUrl;

    fetchSiteIcon(trimmedUrl)
      .then((iconUrl) => {
        if (iconUrl) {
          // Preload the image to ensure it loads successfully
          const img = new Image();
          img.onload = () => {
            setLogo(iconUrl);
            currentIconUrl.current = iconUrl;
          };
          img.onerror = () => {
            console.error("Failed to load site icon:", iconUrl);
            currentIconUrl.current = undefined;
          };
          img.src = iconUrl;
        }
      })
      .catch((err) => {
        console.error("Failed to fetch manifest:", err);
        currentIconUrl.current = undefined;
      });
  };

  // Reset logo when URL is cleared or becomes invalid
  useEffect(() => {
    if (!siteUrl.trim() || !isValidUrl(siteUrl.trim())) {
      setLogo(defaultLogo);
      lastFetchedUrl.current = "";
      currentIconUrl.current = undefined;
    }
  }, [siteUrl]);

  // Shared authorization logic
  const startAuthorization = useCallback(async (urlToAuthorize: string) => {
    setLoading(true);
    setSnackbarOpen(false);

    try {
      // Validate site version first
      const version = await validateSiteVersion(urlToAuthorize);
      console.log("Site version:", version);

      // Generate PKCE pair
      const { codeVerifier, codeChallenge } = await generatePKCEPair();
      // Add "reauthorize:" prefix to state when in reauthorize mode
      const stateValue = isReauthorize
        ? `reauthorize:${randomCryptoString(32)}`
        : randomCryptoString(32);

      // Store PKCE session data for use after OAuth redirect
      pkceSessionRef.current = {
        codeVerifier,
        codeChallenge,
        siteUrl: urlToAuthorize.trim(),
        siteVersion: version,
        siteIcon: currentIconUrl.current,
        state: stateValue,
      };

      // Build and open the authorization URL
      const authUrl = buildAuthorizeUrl(urlToAuthorize.trim(), codeChallenge, stateValue);
      setAuthorizeUrl(authUrl);
      setPageState("waiting");
      await openUrl(authUrl);
    } catch (error) {
      if (isValidationError(error)) {
        setError(t(`addDrive.errors.${error.type}`, error.params));
      } else {
        const message = error instanceof Error ? error.message : String(error);
        setError(t("addDrive.errors.connectionFailed", { message }));
      }
      setSnackbarOpen(true);
    } finally {
      setLoading(false);
    }
  }, [t, isReauthorize]);

  // Auto-start authorization flow when in reauthorize mode
  useEffect(() => {
    if (isReauthorize && !hasInitialized.current) {
      hasInitialized.current = true;
      // Fetch site icon for the reauthorize URL
      fetchSiteIcon(decodedSiteUrl)
        .then((iconUrl) => {
          if (iconUrl) {
            const img = new Image();
            img.onload = () => {
              setLogo(iconUrl);
              currentIconUrl.current = iconUrl;
            };
            img.src = iconUrl;
          }
        })
        .catch((err) => {
          console.error("Failed to fetch manifest:", err);
        });

      // Auto-start the authorization flow
      startAuthorization(decodedSiteUrl);
    }
  }, [isReauthorize, decodedSiteUrl, startAuthorization]);

  // Trigger confetti effect when entering success state
  useEffect(() => {
    if (pageState === "success") {
      confetti({
        particleCount: 100,
        spread: 70,
        origin: { y: 0.6 },
      });
    }
  }, [pageState]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    await startAuthorization(siteUrl);
  };

  const handleOpenAuthorizeUrl = async () => {
    if (authorizeUrl) {
      await openUrl(authorizeUrl);
    }
  };

  const handleBack = () => {
    setAuthorizeUrl(null);
    setPageState("url_input");
    setLocalPath("");
    setLocalPathError("");
    setLocalPathWarning("");
    pkceSessionRef.current = null;
  };

  const handleCloseSnackbar = () => {
    setSnackbarOpen(false);
  };

  const handleBrowseFolder = async () => {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: t("addDrive.selectFolder"),
    });
    if (selected) {
      setLocalPath(selected);
      setLocalPathError("");
      setLocalPathWarning("");
    }
  };

  const validateLocalPath = useCallback(async () => {
    if (isReauthorize) {
      return localPath;
    }

    setValidatingLocalPath(true);
    try {
      const result = await invoke<LocalPathValidation>("validate_local_sync_path", {
        path: localPath,
        driveId: undefined,
      });
      setLocalPathError("");
      setLocalPathWarning(result.warning ?? "");
      if (result.normalized_path !== localPath) {
        setLocalPath(result.normalized_path);
      }
      return result.normalized_path;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setLocalPathError(message);
      setLocalPathWarning("");
      return null;
    } finally {
      setValidatingLocalPath(false);
    }
  }, [isReauthorize, localPath]);

  const handleFinish = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmedDriveName = driveName.trim();
    if (!trimmedDriveName) {
      return;
    }

    let validatedLocalPath = localPath;
    if (!isReauthorize) {
      const result = await validateLocalPath();
      if (!result) {
        return;
      }
      validatedLocalPath = result;
    }

    setPageState("setting_up");

    let tokens: TokenResponse;
    try {
      tokens = await exchangeTokens(
        pkceSessionRef.current!.siteUrl,
        pkceSessionRef.current!.callbackData!.code,
        pkceSessionRef.current!.codeVerifier
      );
    } catch (error) {
      if (isValidationError(error)) {
        setError(t(`addDrive.errors.${error.type}`, error.params));
      } else {
        const message = error instanceof Error ? error.message : String(error);
        setError(t("addDrive.errors.connectionFailed", { message }));
      }
      setPageState("final_setup");
      setSnackbarOpen(true);
      return;
    }

    // Clean the site URL to only include origin (no path or trailing slash)
    const cleanSiteUrl = new URL(pkceSessionRef.current!.siteUrl).origin;

    try {
      await invoke('add_drive', {
        config: {
          site_url: cleanSiteUrl,
          access_token: tokens.access_token,
          refresh_token: tokens.refresh_token,
          access_token_expires: tokens.expires_in,
          refresh_token_expires: tokens.refresh_token_expires_in,
          drive_name: trimmedDriveName,
          local_path: validatedLocalPath,
          remote_path: pkceSessionRef.current!.callbackData!.path,
          user_id: pkceSessionRef.current!.callbackData!.user_id || "",
          drive_id: isReauthorize ? driveId : undefined,
        }
      });
      // Success - switch to success state
      setPageState("success");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(t("addDrive.errors.addDriveFailed", { message }));
      setPageState("final_setup");
      setSnackbarOpen(true);
    }
  }

  const handleOpenDriveAndClose = async () => {
    const pathToOpen = localPath.endsWith('/') || localPath.endsWith('\\') ? localPath : localPath + '/';
    await openPath(pathToOpen);
    await getCurrentWindow().close();
  }

  return (
    <Container maxWidth="sm" sx={{ backgroundColor: isWindows10 ? "#fff" : undefined, minHeight: "100vh" }}>
      <Box
        sx={{
          minHeight: "100vh",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          alignItems: "center",
          py: 4,
        }}
      >
        <Box
          sx={{
            p: 4,
            py: 2,
            width: "100%",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 1,
            borderRadius: 3,
          }}
        >
          {pageState !== "setting_up" && pageState !== "success" && <Box
            component="img"
            src={logo}
            alt="Cloudreve"
            sx={{
              width: 120,
              height: "auto",
              mb: 2,
            }}
          />}

          {pageState === "success" ? (
            // Success state - all done!
            <>
              <Typography
                variant="h1"
                sx={{ fontSize: 64, mt: 2 }}
              >
                🎉
              </Typography>
              <Typography
                sx={{ mt: 2 }}
                variant="h5"
                component="h1"
                fontWeight={500}
                textAlign="center"
              >
                {isReauthorize ? t("addDrive.reauthorizeSuccessTitle") : t("addDrive.successTitle")}
              </Typography>

              <Box
                sx={{
                  width: "100%",
                  display: "flex",
                  flexDirection: "column",
                  gap: 2,
                  mt: 2,
                }}
              >
                {isReauthorize ? (
                  <Button
                    variant="contained"
                    size="large"
                    fullWidth
                    onClick={() => getCurrentWindow().close()}
                  >
                    {t("addDrive.close")}
                  </Button>
                ) : (
                  <Button
                    variant="contained"
                    size="large"
                    fullWidth
                    onClick={handleOpenDriveAndClose}
                  >
                    {t("addDrive.openDrive", { name: driveName })}
                  </Button>
                )}
              </Box>
            </>
          ) : pageState === "setting_up" ? (
            // Setting up state - loading indicator
            <>
              <CircularProgress size={48} sx={{ mt: 2 }} />
              <Typography
                sx={{ mt: 2 }}
                variant="h6"
                component="h1"
                fontWeight={500}
                textAlign="center"
              >
                {t("addDrive.settingUp")}
              </Typography>
            </>
          ) : pageState === "final_setup" ? (
            // Final setup state - local path input (hidden in reauthorize mode)
            <>
              <Typography
                sx={{ mt: 2 }}
                variant="h5"
                component="h1"
                fontWeight={500}
              >
                {isReauthorize ? t("addDrive.reauthorizeTitle") : t("addDrive.finalSetupTitle")}
              </Typography>

              <Typography
                variant="body2"
                color="text.secondary"
                textAlign="center"
              >
                {isReauthorize ? t("addDrive.reauthorizeDescription") : t("addDrive.finalSetupDescription")}
              </Typography>

              {!isReauthorize && (
                <Typography
                  variant="caption"
                  color="text.secondary"
                  textAlign="center"
                  sx={{ display: "block", px: 1 }}
                >
                  {t("addDrive.stepProgress", { current: 3, total: 3 })} · {t("addDrive.step3Body")}
                </Typography>
              )}

              {!isReauthorize && (
                <Typography
                  variant="caption"
                  color="text.secondary"
                  textAlign="center"
                  sx={{ display: "block", px: 1, mt: -0.5 }}
                >
                  {localPathTips}
                </Typography>
              )}

              <Box
                component="form"
                onSubmit={handleFinish}
                sx={{
                  width: "100%",
                  display: "flex",
                  flexDirection: "column",
                  gap: 2,
                  mt: 2,
                }}
              >
                <FilledTextField
                  disabled={mode === "reauthorize"}
                  fullWidth
                  autoComplete="off"
                  label={t("addDrive.localDriveName")}
                  value={driveName}
                  onChange={(e) => setDriveName(e.target.value)}
                  variant="filled"
                  required
                />

                {!isReauthorize && (
                  <FilledTextField
                    fullWidth
                    autoComplete="off"
                    label={t("addDrive.localPath")}
                    placeholder={localPathPlaceholder}
                    value={localPath}
                    onChange={(e) => {
                      setLocalPath(e.target.value);
                      setLocalPathError("");
                      setLocalPathWarning("");
                    }}
                    variant="filled"
                    required
                    error={Boolean(localPathError)}
                    helperText={localPathError || localPathWarning || " "}
                    slotProps={{
                      input: {
                        endAdornment: (
                          <InputAdornment position="end">
                            <Button
                              onClick={handleBrowseFolder}
                              size="small"
                            >
                              {t("addDrive.browse")}
                            </Button>
                          </InputAdornment>
                        ),
                      },
                    }}
                  />
                )}

                <Button
                  type="submit"
                  variant="contained"
                  size="large"
                  fullWidth
                  disabled={validatingLocalPath || (!isReauthorize && !localPath.trim()) || !driveName.trim()}
                >
                  {isReauthorize ? t("addDrive.reauthorizeConfirm") : t("addDrive.finish")}
                </Button>

                <Button
                  variant="text"
                  size="large"
                  fullWidth
                  onClick={handleBack}
                >
                  {t("addDrive.back")}
                </Button>
              </Box>
            </>
          ) : pageState === "waiting" ? (
            // Waiting for sign-in state
            <>
              <Typography
                sx={{ mt: 2 }}
                variant="h5"
                component="h1"
                fontWeight={500}
              >
                {t("addDrive.waitingTitle")}
              </Typography>

              <Typography
                variant="body2"
                color="text.secondary"
                textAlign="center"
              >
                {t("addDrive.waitingDescription")}
              </Typography>

              {isReauthorize ? (
                <Typography
                  variant="caption"
                  color="text.secondary"
                  textAlign="center"
                  sx={{ display: "block", px: 1 }}
                >
                  {t("addDrive.reauthorizeWaitingHint")}
                </Typography>
              ) : (
                <Typography
                  variant="caption"
                  color="text.secondary"
                  textAlign="center"
                  sx={{ display: "block", px: 1 }}
                >
                  {t("addDrive.stepProgress", { current: 2, total: 3 })} · {t("addDrive.step2Body")}
                </Typography>
              )}

              {osPlatform === "macos" && (
                <Typography
                  variant="caption"
                  color="warning.main"
                  textAlign="center"
                  sx={{ display: "block", px: 1 }}
                >
                  {t("addDrive.waitingDeepLinkMac")}
                </Typography>
              )}

              <Box
                sx={{
                  width: "100%",
                  display: "flex",
                  flexDirection: "column",
                  gap: 2,
                  mt: 2,
                }}
              >
                <Button
                  variant="contained"
                  size="large"
                  fullWidth
                  onClick={handleOpenAuthorizeUrl}
                >
                  {t("addDrive.reopenBrowser")}
                </Button>

                <Button
                  variant="text"
                  size="large"
                  fullWidth
                  onClick={handleBack}
                >
                  {t("addDrive.back")}
                </Button>
              </Box>
            </>
          ) : (
            // Initial URL input state
            <>
              <Typography
                sx={{ mt: 2 }}
                variant="h5"
                component="h1"
                fontWeight={500}
              >
                {t("addDrive.title")}
              </Typography>

              <Typography
                variant="body2"
                color="text.secondary"
                textAlign="center"
              >
                {t("addDrive.description")}
              </Typography>

              {!isReauthorize && (
                <Typography
                  variant="caption"
                  color="text.secondary"
                  textAlign="center"
                  sx={{ display: "block", px: 1 }}
                >
                  {t("addDrive.stepProgress", { current: 1, total: 3 })} · {t("addDrive.step1Body")}
                </Typography>
              )}

              <Box
                component="form"
                onSubmit={handleSubmit}
                sx={{
                  width: "100%",
                  display: "flex",
                  flexDirection: "column",
                  gap: 2,
                  mt: 2,
                }}
              >
                <FilledTextField
                  fullWidth
                  autoComplete="off"
                  slotProps={{
                    input: {
                      readOnly: loading,
                    },
                  }}
                  label={t("addDrive.siteUrl")}
                  placeholder={t("addDrive.siteUrlPlaceholder")}
                  value={siteUrl}
                  onChange={(e) => setSiteUrl(e.target.value)}
                  onBlur={handleUrlBlur}
                  variant="filled"
                  type="url"
                  required
                />

                <Button
                  type="submit"
                  variant="contained"
                  size="large"
                  loading={loading}
                  fullWidth
                >
                  {t("addDrive.connect")}
                </Button>
              </Box>
            </>
          )}
        </Box>
      </Box>
      <Snackbar
        open={snackbarOpen}
        autoHideDuration={8000}
        onClose={handleCloseSnackbar}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert
          onClose={handleCloseSnackbar}
          severity="error"
          variant="filled"
          sx={{ width: "100%", maxWidth: 480 }}
        >
          {error}
        </Alert>
      </Snackbar>
    </Container>
  );
}
