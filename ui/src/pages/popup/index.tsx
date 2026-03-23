import {
  Alert,
  Box,
  IconButton,
  List,
  Snackbar,
  Typography,
  Divider,
} from "@mui/material";
import {
  Folder as FolderIcon,
  CheckCircle as CheckCircleIcon,
  Refresh as RefreshIcon,
  WarningAmber as WarningAmberIcon,
} from "@mui/icons-material";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import Settings from "../../common/icons/Settings";
import CloudreveLogo from "../../common/CloudreveLogo";
import type { StatusSummary } from "./types";
import type { DriveInfo } from "../settings/types";
import DriveChips from "./DriveChips";
import DriveStatusOverview from "./DriveStatusOverview";
import TaskItem from "./TaskItem";

export default function Popup() {
  const { t } = useTranslation();
  const [summary, setSummary] = useState<StatusSummary | null>(null);
  const [driveInfos, setDriveInfos] = useState<DriveInfo[]>([]);
  const [selectedDrive, setSelectedDrive] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [uiError, setUiError] = useState("");
  const isFetchingRef = useRef(false);

  // Close window on blur (when it loses focus)
  useEffect(() => {
    let unlisten: () => void;
    const currentWindow = getCurrentWindow();

    currentWindow
      .onFocusChanged(({ payload: focused }) => {
        if (!focused) {
          currentWindow.close();
        }
      })
      .then((fn) => {
        unlisten = fn;
      });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  // Fetch status summary
  const fetchSummary = useCallback(async () => {
    if (isFetchingRef.current) return;

    isFetchingRef.current = true;
    try {
      const [summaryResult, drivesResult] = await Promise.all([
        invoke<StatusSummary>("get_status_summary", {
          driveId: null,
        }),
        invoke<DriveInfo[]>("get_drives_info"),
      ]);
      setSummary(summaryResult);
      setDriveInfos(
        drivesResult.map((drive) => ({
          ...drive,
          status: drive.status as DriveInfo["status"],
        }))
      );
    } catch (error) {
      console.error("Failed to fetch status summary:", error);
      setUiError(t("popup.loadFailed", "Failed to load sync status."));
    } finally {
      isFetchingRef.current = false;
      setLoading(false);
    }
  }, []);

  // Initial fetch and polling
  useEffect(() => {
    fetchSummary();

    const intervalId = setInterval(() => {
      fetchSummary();
    }, 1000);

    return () => {
      clearInterval(intervalId);
    };
  }, [fetchSummary]);

  const handleDriveSelect = (driveId: string | null) => {
    setSelectedDrive(driveId);
  };

  const handleAddDrive = async () => {
    try {
      await invoke("show_add_drive_window");
    } catch (error) {
      console.error("Failed to open add drive window:", error);
      setUiError(t("popup.openAddDriveFailed", "Failed to open the add drive window."));
    }
  };

  const handleSettings = async () => {
    try {
      await invoke("show_settings_window");
    } catch (error) {
      console.error("Failed to open settings window:", error);
      setUiError(t("popup.openSettingsFailed", "Failed to open settings."));
    }
  };

  const handleOpenFolder = async (path: string) => {
    try {
      await invoke("show_file_in_explorer", { path });
    } catch (error) {
      console.error("Failed to open folder:", error);
      const message = error instanceof Error ? error.message : String(error);
      setUiError(message);
    }
  };

  const handleReauthorize = async (drive: DriveInfo) => {
    try {
      await invoke("show_reauthorize_window", {
        driveId: drive.id,
        siteUrl: drive.instance_url,
        driveName: drive.name,
      });
    } catch (error) {
      console.error("Failed to open reauthorize window:", error);
      setUiError(t("popup.openReauthorizeFailed", "Failed to open reauthorization."));
    }
  };

  const displayedActiveTasks = selectedDrive
    ? (summary?.active_tasks ?? []).filter((task) => task.drive_id === selectedDrive)
    : (summary?.active_tasks ?? []);
  const displayedFinishedTasks = selectedDrive
    ? (summary?.finished_tasks ?? []).filter((task) => task.drive_id === selectedDrive)
    : (summary?.finished_tasks ?? []);

  const hasActiveTasks =
    displayedActiveTasks.length > 0;
  const hasFinishedTasks =
    displayedFinishedTasks.length > 0;
  const selectedDriveInfo = selectedDrive
    ? driveInfos.find((drive) => drive.id === selectedDrive) ?? null
    : null;
  const issueDrives = driveInfos.filter((drive) => drive.status !== "active");
  const hasSelectedDriveIssue = selectedDriveInfo && selectedDriveInfo.status !== "active";

  return (
    <Box
      sx={{
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        bgcolor: "background.paper",
        overflow: "hidden",
      }}
    >
      {/* Header */}
      <Box
        sx={{
          px: 2,
          pt: 1.5,
          pb: 1,
          borderBottom: 1,
          borderColor: "divider",
          backgroundColor: (theme) =>
            theme.palette.mode === "light" ? theme.palette.grey[100] : theme.palette.grey[900],
        }}
      >
        {/* Top row: Logo and Settings */}
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            mb: 1.5,
          }}
        >
          <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
            <CloudreveLogo height={28} />
          </Box>
          <IconButton size="small" onClick={handleSettings}>
            <Settings fontSize="small" />
          </IconButton>
        </Box>

        {/* Drive filter chips */}
        <DriveChips
          drives={summary?.drives ?? []}
          selectedDrive={selectedDrive}
          onDriveSelect={handleDriveSelect}
          onAddDrive={handleAddDrive}
        />
      </Box>

      <DriveStatusOverview
        drives={driveInfos}
        selectedDrive={selectedDrive}
        activeTasks={summary?.active_tasks ?? []}
        finishedTasks={summary?.finished_tasks ?? []}
        onOpenFolder={handleOpenFolder}
        onReauthorize={handleReauthorize}
      />

      {/* Task List */}
      <Box sx={{ flex: 1, overflow: "auto" }}>
        {loading ? (
          <Box
            sx={{
              display: "flex",
              justifyContent: "center",
              alignItems: "center",
              height: "100%",
            }}
          >
            <Typography variant="body2" color="text.secondary">
              {t("popup.loading", "Loading...")}
            </Typography>
          </Box>
        ) : !hasActiveTasks && !hasFinishedTasks ? (
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              justifyContent: "center",
              alignItems: "center",
              height: "100%",
              gap: 1,
            }}
          >
            <FolderIcon sx={{ fontSize: 48, color: "text.disabled" }} />
            <Typography variant="body2" color="text.secondary">
              {t("popup.noActivity", "No recent activity")}
            </Typography>
          </Box>
        ) : (
          <List disablePadding>
            {/* Active Tasks */}
            {hasActiveTasks && (
              <>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{
                    px: 2,
                    py: 1,
                    pb:0,
                    display: "block",
                    fontWeight: 600,
                    textTransform: "uppercase",
                  }}
                >
                  {t("popup.syncing", "Syncing")}
                </Typography>
                {displayedActiveTasks.map((task) => (
                  <TaskItem
                    key={task.id}
                    task={task}
                    isActive
                    onRevealPath={handleOpenFolder}
                  />
                ))}
              </>
            )}

            {/* Divider between active and finished */}
            {hasActiveTasks && hasFinishedTasks && (
              <Divider sx={{ my: 1 }} />
            )}

            {/* Finished Tasks */}
            {hasFinishedTasks && (
              <>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{
                    px: 2,
                    py: 1,
                    pb:0,
                    display: "block",
                    fontWeight: 600,
                    textTransform: "uppercase",
                  }}
                >
                  {t("popup.recent", "Recent")}
                </Typography>
                {displayedFinishedTasks.map((task) => (
                  <TaskItem
                    key={task.id}
                    task={task}
                    onRevealPath={handleOpenFolder}
                  />
                ))}
              </>
            )}
          </List>
        )}
      </Box>

      {/* Footer Status */}
      <Box
        sx={{
          px: 2,
          py: 1,
          borderTop: 1,
          borderColor: "divider",
          display: "flex",
          alignItems: "center",
          gap: 1,
        }}
      >
        {hasActiveTasks ? (
          <RefreshIcon
            sx={{
              fontSize: 18,
              color: "primary.main",
              animation: "spin 1s linear infinite",
              "@keyframes spin": {
                "0%": { transform: "rotate(0deg)" },
                "100%": { transform: "rotate(360deg)" },
              },
            }}
          />
        ) : hasSelectedDriveIssue || issueDrives.length > 0 ? (
          <WarningAmberIcon sx={{ fontSize: 18, color: "warning.main" }} />
        ) : (
          <CheckCircleIcon
            sx={{ fontSize: 18, color: "success.main" }}
          />
        )}
        <Typography variant="caption" color="text.secondary">
          {hasActiveTasks
            ? t("popup.syncingStatus", "Syncing {{count}} file(s)...", {
                count: displayedActiveTasks.length ?? 0,
              })
            : hasSelectedDriveIssue
              ? selectedDriveInfo?.status === "credential_expired"
                ? t("popup.driveNeedsReauth", "This drive needs reauthorization")
                : t("popup.driveNeedsAttention", "This drive needs attention")
              : issueDrives.length > 0
                ? t("popup.drivesNeedAttention", {
                    count: issueDrives.length,
                    defaultValue: "{{count}} drive(s) need attention",
                  })
            : t("popup.upToDate", "Your files are up to date")}
        </Typography>
      </Box>
      <Snackbar
        open={Boolean(uiError)}
        autoHideDuration={3500}
        onClose={() => setUiError("")}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity="error" onClose={() => setUiError("")} sx={{ width: "100%" }}>
          {uiError}
        </Alert>
      </Snackbar>
    </Box>
  );
}
