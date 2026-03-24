import {
  Box,
  IconButton,
  LinearProgress,
  Link,
  ListItem,
  ListItemIcon,
  ListItemText,
  Menu,
  MenuItem,
  Tooltip,
  Typography,
} from "@mui/material";
import {
  CheckCircle as CheckCircleIcon,
  Error as ErrorIcon,
  CloudUpload as UploadIcon,
  CloudDownload as DownloadIcon,
  MoreVert as MoreVertIcon,
  Refresh as RefreshIcon,
} from "@mui/icons-material";
import { invoke } from "@tauri-apps/api/core";
import TimeAgo from "react-timeago";
import { useTranslation } from "react-i18next";
import { useState } from "react";
import type { TaskWithProgress, TaskRecord } from "./types";
import { getFriendlyTaskError } from "./taskErrorMessage";
import { formatBytes, getFileName, getParentFolderName } from "./utils";
import FileIcon from "./FileIcon";

interface TaskItemProps {
  task: TaskWithProgress | TaskRecord;
  isActive?: boolean;
  onRevealPath?: (path: string) => void;
  /** Refresh list after user resolves a conflict (keep remote / overwrite / save as new). */
  onConflictResolved?: () => void;
  onSyncActionError?: (message: string) => void;
  /** Re-enqueue upload/download after failure (e.g. network timeout). */
  onRetry?: () => void;
}

/** Failed upload tasks whose error text indicates a version / name conflict (same heuristics as friendly messages). */
function taskErrorLooksLikeSyncConflict(error: string | undefined): boolean {
  if (!error) return false;
  const n = error.toLowerCase();
  if (n.includes("credential") && n.includes("expired")) return false;
  if (n.includes("unauthorized") || (n.includes("401") && n.includes("login"))) return false;
  return (
    n.includes("40076") ||
    n.includes("40004") ||
    n.includes("stale") ||
    n.includes("conflict") ||
    n.includes("object existed") ||
    n.includes("modification time") ||
    n.includes("metadata mismatch")
  );
}

export default function TaskItem({
  task,
  isActive = false,
  onRevealPath,
  onConflictResolved,
  onSyncActionError,
  onRetry,
}: TaskItemProps) {
  const { t } = useTranslation("common");
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [conflictMenuAnchor, setConflictMenuAnchor] = useState<null | HTMLElement>(null);
  const activeTask = task as TaskWithProgress;
  const liveProgress = activeTask.live_progress;
  const progress = liveProgress?.progress ?? task.progress;
  const isUpload = task.task_type === "upload";
  const fileName = getFileName(task.local_path);
  const parentFolderName = getParentFolderName(task.local_path);
  const isFailed = task.status === "Failed";
  const isCancelled = task.status === "Cancelled";
  const showRetry =
    Boolean(onRetry) &&
    !isActive &&
    (task.status === "Failed" || task.status === "Cancelled") &&
    (task.task_type === "upload" || task.task_type === "download");
  const showFinishedErrorRow =
    (isFailed || isCancelled) && (Boolean(task.error) || showRetry);
  const showConflictActions =
    Boolean(onConflictResolved) &&
    isFailed &&
    isUpload &&
    taskErrorLooksLikeSyncConflict(task.error);

  const runRetry = async () => {
    if (!onRetry) return;
    setBusyAction("retry");
    try {
      await invoke("retry_sync_task", {
        driveId: task.drive_id,
        localPath: task.local_path,
        taskType: task.task_type,
      });
      onRetry();
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      onSyncActionError?.(message);
      console.error("retry_sync_task", err);
    } finally {
      setBusyAction(null);
    }
  };

  const runConflictAction = async (action: "keep_remote" | "overwrite_remote" | "save_as_new") => {
    setConflictMenuAnchor(null);
    setBusyAction(action);
    try {
      await invoke("resolve_sync_conflict", {
        driveId: task.drive_id,
        localPath: task.local_path,
        action,
      });
      onConflictResolved?.();
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      onSyncActionError?.(message);
      console.error("resolve_sync_conflict", err);
    } finally {
      setBusyAction(null);
    }
  };

  const timeAgoFormatter = (
    value: number,
    unit: string,
    suffix: string
  ): string => {
    if (unit === "second") {
      return t("timeAgo.justNow", "Just now");
    }
    const unitKey = value === 1 ? unit : `${unit}s`;
    return t(`timeAgo.${unitKey}${suffix === "ago" ? "Ago" : "FromNow"}`, `${value} ${unitKey} ${suffix}`, { value });
  };

  const handleShowInExplorer = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (onRevealPath) {
      onRevealPath(task.local_path);
      return;
    }
    invoke("show_file_in_explorer", { path: task.local_path });
  };

  const getStatusBadge = () => {
    if (isActive) {
      return isUpload ? (
        <UploadIcon sx={{ fontSize: 14 }} color="primary" />
      ) : (
        <DownloadIcon sx={{ fontSize: 14 }} color="primary" />
      );
    }
    switch (task.status) {
      case "Completed":
        return <CheckCircleIcon sx={{ fontSize: 14 }} color="success" />;
      case "Failed":
      case "Cancelled":
        return <ErrorIcon sx={{ fontSize: 14 }} color="error" />;
      default:
        return null;
    }
  };

  const getSecondaryText = () => {
    if (isActive && liveProgress) {
      const processed = formatBytes(liveProgress.processed_bytes ?? 0);
      const total = formatBytes(liveProgress.total_bytes ?? 0);
      const speed = formatBytes(liveProgress.speed_bytes_per_sec);
      return `${processed} / ${total} - ${speed}/s`;
    }
    if (isActive) {
      return task.status === "Pending"
        ? t("popup.waiting", "Waiting...")
        : t("popup.processing", "Processing...");
    }
    return null;
  };

  const statusBadge = getStatusBadge();
  const secondaryText = getSecondaryText();

  return (
    <ListItem
      sx={{
        px: 2,
        py: 1,
        "&:hover": {
          bgcolor: "action.hover",
        },
      }}
    >
      <ListItemIcon sx={{ minWidth: 40 }}>
        <Box sx={{ position: "relative", width: 28, height: 28 }}>
          <FileIcon path={task.local_path} size={28} />
          {statusBadge && (
            <Box
              sx={{
                position: "absolute",
                bottom: -4,
                right: -4,
                bgcolor: "background.paper",
                borderRadius: "50%",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                width: 18,
                height: 18,
              }}
            >
              {statusBadge}
            </Box>
          )}
        </Box>
      </ListItemIcon>
      <ListItemText
        primary={
          <Typography variant="body2" noWrap sx={{ fontWeight: 500 }}>
            {fileName}
          </Typography>
        }
        secondary={
          <Box>
            {showFinishedErrorRow ? (
              <>
                <Box
                  sx={{
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 0.25,
                    mt: 0.25,
                  }}
                >
                  <Typography
                    variant="caption"
                    color="error"
                    component="span"
                    title={task.error ?? undefined}
                    sx={{ flex: 1, minWidth: 0 }}
                  >
                    {task.error
                      ? getFriendlyTaskError(task.error, t)
                      : isFailed
                        ? t("popup.taskFailedBrief", "Failed")
                        : t("popup.taskCancelledBrief", "Cancelled")}
                  </Typography>
                  {showRetry && (
                    <Tooltip title={t("popup.retryTask", "Retry")}>
                      <span>
                        <IconButton
                          size="small"
                          aria-label={t("popup.retryTask", "Retry")}
                          disabled={busyAction !== null}
                          onClick={(e) => {
                            e.stopPropagation();
                            void runRetry();
                          }}
                          sx={{ p: 0.25, mt: -0.25, flexShrink: 0 }}
                        >
                          <RefreshIcon
                            sx={{
                              fontSize: 18,
                              animation:
                                busyAction === "retry"
                                  ? "spin 0.8s linear infinite"
                                  : undefined,
                              "@keyframes spin": {
                                "0%": { transform: "rotate(0deg)" },
                                "100%": { transform: "rotate(360deg)" },
                              },
                            }}
                          />
                        </IconButton>
                      </span>
                    </Tooltip>
                  )}
                  {showConflictActions && (
                    <>
                      <IconButton
                        size="small"
                        aria-label={t(
                          "popup.conflictResolveMenuLabel",
                          "Conflict resolution options"
                        )}
                        aria-haspopup="menu"
                        aria-expanded={Boolean(conflictMenuAnchor)}
                        disabled={busyAction !== null}
                        onClick={(e) => {
                          e.stopPropagation();
                          setConflictMenuAnchor(e.currentTarget);
                        }}
                        sx={{ p: 0.25, mt: -0.25, flexShrink: 0 }}
                      >
                        <MoreVertIcon sx={{ fontSize: 18 }} />
                      </IconButton>
                      <Menu
                        anchorEl={conflictMenuAnchor}
                        open={Boolean(conflictMenuAnchor)}
                        onClose={() => setConflictMenuAnchor(null)}
                        anchorOrigin={{ vertical: "bottom", horizontal: "right" }}
                        transformOrigin={{ vertical: "top", horizontal: "right" }}
                        MenuListProps={{ dense: true }}
                      >
                        <MenuItem
                          onClick={() => void runConflictAction("keep_remote")}
                          disabled={busyAction !== null}
                        >
                          {busyAction === "keep_remote"
                            ? t("popup.conflictResolving", "…")
                            : t("popup.conflictActionKeepRemote", "Use cloud version")}
                        </MenuItem>
                        <MenuItem
                          onClick={() => void runConflictAction("overwrite_remote")}
                          disabled={busyAction !== null}
                        >
                          {busyAction === "overwrite_remote"
                            ? t("popup.conflictResolving", "…")
                            : t("popup.conflictActionOverwriteRemote", "Overwrite cloud")}
                        </MenuItem>
                        <MenuItem
                          onClick={() => void runConflictAction("save_as_new")}
                          disabled={busyAction !== null}
                        >
                          {busyAction === "save_as_new"
                            ? t("popup.conflictResolving", "…")
                            : t("popup.conflictActionSaveAsNew", "Save as new file")}
                        </MenuItem>
                      </Menu>
                    </>
                  )}
                </Box>
              </>
            ) : (
              <Typography variant="caption" color="text.secondary" component="span">
                {secondaryText ?? (
                  <TimeAgo
                    date={task.updated_at * 1000}
                    formatter={timeAgoFormatter}
                  />
                )}
              </Typography>
            )}
            {!isActive && (
              <>
                <Typography variant="caption" color="text.secondary" component="span">
                  {" · "}
                </Typography>
                <Link
                  component="button"
                  variant="caption"
                  color="text.secondary"
                  onClick={handleShowInExplorer}
                  underline="always"
                  sx={{
                  }}
                >
                  {parentFolderName}
                </Link>
              </>
            )}
            {isActive && (
              <LinearProgress
                variant="determinate"
                value={progress * 100}
                sx={{ mt: 0.5, height: 4, borderRadius: 2 }}
              />
            )}
          </Box>
        }
      />
    </ListItem>
  );
}
