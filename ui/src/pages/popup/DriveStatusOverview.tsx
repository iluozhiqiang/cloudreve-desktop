import {
  Alert,
  Box,
  Button,
  Chip,
  Stack,
  Typography,
} from "@mui/material";
import {
  CheckCircleOutline as CheckCircleOutlineIcon,
  ErrorOutline as ErrorOutlineIcon,
  FolderOpen as FolderOpenIcon,
  Refresh as RefreshIcon,
  WarningAmber as WarningAmberIcon,
} from "@mui/icons-material";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { DriveInfo } from "../settings/types";
import type { TaskRecord, TaskWithProgress } from "./types";

interface DriveStatusOverviewProps {
  drives: DriveInfo[];
  selectedDrive: string | null;
  activeTasks: TaskWithProgress[];
  finishedTasks: TaskRecord[];
  onOpenFolder: (path: string) => void;
  onReauthorize: (drive: DriveInfo) => void;
}

function getStatusColor(status: DriveInfo["status"]) {
  switch (status) {
    case "active":
      return "success";
    case "event_push_lost":
      return "warning";
    case "credential_expired":
      return "error";
    default:
      return "default";
  }
}

function getStatusLabel(status: DriveInfo["status"], t: ReturnType<typeof useTranslation>["t"]) {
  switch (status) {
    case "active":
      return t("settings.driveStatus.active");
    case "event_push_lost":
      return t("settings.driveStatus.eventPushLost");
    case "credential_expired":
      return t("settings.driveStatus.credentialExpired");
    default:
      return status;
  }
}

function getIssueMessage(drive: DriveInfo, t: ReturnType<typeof useTranslation>["t"]) {
  switch (drive.status) {
    case "credential_expired":
      return t(
        "popup.driveCredentialExpiredHint",
        "This drive needs reauthorization before sync can continue."
      );
    case "event_push_lost":
      return t(
        "popup.driveEventPushLostHint",
        "Real time push is unavailable right now. The app will rely more on polling."
      );
    default:
      return null;
  }
}

export default function DriveStatusOverview({
  drives,
  selectedDrive,
  activeTasks,
  finishedTasks,
  onOpenFolder,
  onReauthorize,
}: DriveStatusOverviewProps) {
  const { t } = useTranslation();

  const driveTaskStats = useMemo(() => {
    const stats = new Map<
      string,
      { active: number; failed: number; lastFailed?: TaskRecord }
    >();

    const ensure = (driveId: string) => {
      if (!stats.has(driveId)) {
        stats.set(driveId, { active: 0, failed: 0 });
      }
      return stats.get(driveId)!;
    };

    activeTasks.forEach((task) => {
      ensure(task.drive_id).active += 1;
    });

    finishedTasks.forEach((task) => {
      if (task.status !== "Failed") {
        return;
      }
      const entry = ensure(task.drive_id);
      entry.failed += 1;
      if (!entry.lastFailed || task.updated_at > entry.lastFailed.updated_at) {
        entry.lastFailed = task;
      }
    });

    return stats;
  }, [activeTasks, finishedTasks]);

  const issueDrives = drives.filter((drive) => drive.status !== "active");
  const selectedDriveInfo = selectedDrive
    ? drives.find((drive) => drive.id === selectedDrive) ?? null
    : null;
  const activeTaskCount = activeTasks.length;

  if (drives.length === 0) {
    return null;
  }

  if (!selectedDriveInfo) {
    return (
      <Box sx={{ px: 2, py: 1.5, borderBottom: 1, borderColor: "divider" }}>
        <Stack direction="row" spacing={1}>
          <Box
            sx={{
              flex: 1,
              p: 1,
              borderRadius: 1.5,
              bgcolor: "action.hover",
            }}
          >
            <Typography variant="caption" color="text.secondary">
              {t("popup.drivesCount", "Drives")}
            </Typography>
            <Typography variant="body2" fontWeight={600}>
              {drives.length}
            </Typography>
          </Box>
          <Box
            sx={{
              flex: 1,
              p: 1,
              borderRadius: 1.5,
              bgcolor: "action.hover",
            }}
          >
            <Typography variant="caption" color="text.secondary">
              {t("popup.activeTasks", "Active tasks")}
            </Typography>
            <Typography variant="body2" fontWeight={600}>
              {activeTaskCount}
            </Typography>
          </Box>
          <Box
            sx={{
              flex: 1,
              p: 1,
              borderRadius: 1.5,
              bgcolor: issueDrives.length > 0 ? "warning.light" : "action.hover",
              color: issueDrives.length > 0 ? "warning.contrastText" : "text.primary",
            }}
          >
            <Typography
              variant="caption"
              color={issueDrives.length > 0 ? "inherit" : "text.secondary"}
            >
              {t("popup.needsAttention", "Needs attention")}
            </Typography>
            <Typography variant="body2" fontWeight={600}>
              {issueDrives.length}
            </Typography>
          </Box>
        </Stack>
        {issueDrives.length > 0 && (
          <Alert severity="warning" sx={{ mt: 1.25, py: 0 }}>
            <Typography variant="caption">
              {t("popup.issueDrivesHint", {
                count: issueDrives.length,
                defaultValue: "{{count}} drive(s) need attention. Select a drive above to inspect it.",
              })}
            </Typography>
          </Alert>
        )}
      </Box>
    );
  }

  const stats = driveTaskStats.get(selectedDriveInfo.id) ?? { active: 0, failed: 0 };
  const issueMessage = getIssueMessage(selectedDriveInfo, t);

  return (
    <Box sx={{ px: 2, py: 1.5, borderBottom: 1, borderColor: "divider" }}>
      <Stack spacing={1}>
        <Box sx={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 1 }}>
          <Box sx={{ minWidth: 0 }}>
            <Typography variant="body2" fontWeight={600} noWrap>
              {selectedDriveInfo.name}
            </Typography>
            <Typography variant="caption" color="text.secondary" noWrap>
              {selectedDriveInfo.sync_path}
            </Typography>
          </Box>
          <Chip
            size="small"
            color={getStatusColor(selectedDriveInfo.status)}
            label={getStatusLabel(selectedDriveInfo.status, t)}
          />
        </Box>

        <Stack direction="row" spacing={1}>
          <Chip
            size="small"
            variant="outlined"
            icon={
              stats.active > 0 ? (
                <RefreshIcon sx={{ fontSize: 16 }} />
              ) : (
                <CheckCircleOutlineIcon sx={{ fontSize: 16 }} />
              )
            }
            label={t("popup.activeTasksCount", {
              count: stats.active,
              defaultValue: "{{count}} active task(s)",
            })}
          />
          <Chip
            size="small"
            variant="outlined"
            icon={<ErrorOutlineIcon sx={{ fontSize: 16 }} />}
            label={t("popup.failedTasksCount", {
              count: stats.failed,
              defaultValue: "{{count}} recent failure(s)",
            })}
          />
        </Stack>

        {issueMessage && (
          <Alert
            severity={selectedDriveInfo.status === "credential_expired" ? "error" : "warning"}
            sx={{ py: 0 }}
          >
            <Typography variant="caption">{issueMessage}</Typography>
          </Alert>
        )}

        {stats.lastFailed?.error && (
          <Alert severity="error" sx={{ py: 0 }}>
            <Typography variant="caption" sx={{ wordBreak: "break-word" }}>
              {stats.lastFailed.error}
            </Typography>
          </Alert>
        )}

        <Stack direction="row" spacing={1}>
          <Button
            size="small"
            variant="text"
            startIcon={<FolderOpenIcon />}
            onClick={() => onOpenFolder(selectedDriveInfo.sync_path)}
          >
            {t("settings.openFolder")}
          </Button>
          {selectedDriveInfo.status === "credential_expired" && (
            <Button
              size="small"
              variant="text"
              color="error"
              startIcon={<WarningAmberIcon />}
              onClick={() => onReauthorize(selectedDriveInfo)}
            >
              {t("settings.reauthorize")}
            </Button>
          )}
        </Stack>
      </Stack>
    </Box>
  );
}
