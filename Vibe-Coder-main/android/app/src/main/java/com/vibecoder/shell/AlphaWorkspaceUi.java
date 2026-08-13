package com.vibecoder.shell;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Build;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * Part 34.10 mobile UI shell.
 *
 * This class stays presentation-only: persisted conversation mutation and AI execution remain
 * Rust-owned behind MainActivity/JNI callbacks. The old-chat drawer may read already-persisted JSON
 * for bounded display, while Send/New Chat/Stop are delegated to the native controller. The Preview
 * tab remains an honest placeholder until the local preview runtime is connected.
 */
final class AlphaWorkspaceUi {
    interface Callbacks {
        void onOpenDiagnostics();
        void onNewChatRequested();
        void onConversationSelectionCleared();
        void onConversationSelected(String projectId, String conversationId);
        void onSendRequested(String prompt);
        void onStopRequested();
    }

    private static final int COLOR_BG = Color.rgb(249, 250, 255);
    private static final int COLOR_SURFACE = Color.WHITE;
    private static final int COLOR_TEXT = Color.rgb(25, 31, 52);
    private static final int COLOR_MUTED = Color.rgb(104, 112, 139);
    private static final int COLOR_PRIMARY = Color.rgb(79, 70, 229);
    private static final int COLOR_PRIMARY_SOFT = Color.rgb(239, 239, 255);
    private static final int COLOR_BORDER = Color.rgb(229, 232, 242);
    private static final int COLOR_ASSISTANT = Color.rgb(246, 247, 251);
    private static final int MAX_CHAT_FILES = 50;
    // Must not hide conversations that are valid under the Rust persistence contract.
    private static final int MAX_CHAT_FILE_BYTES = 16 * 1024 * 1024;
    private static final int MAX_CONVERSATION_MESSAGES = 4096;
    private static final int MAX_CONVERSATION_MESSAGE_BYTES = 256 * 1024;
    private static final int MAX_CONVERSATION_TEXT_BYTES = 12 * 1024 * 1024;
    private static final int MAX_CONVERSATION_TITLE_BYTES = 200;
    private static final int MAX_RENDERED_MESSAGES = 80;
    private static final int MAX_RENDERED_TEXT_BYTES = 512 * 1024;
    private static final int MAX_RENDERED_SINGLE_MESSAGE_BYTES = 64 * 1024;

    private final Activity activity;
    private final Callbacks callbacks;
    private final FrameLayout root;
    private final LinearLayout drawerList;
    private final FrameLayout drawerOverlay;
    private final LinearLayout chatMessages;
    private final ScrollView chatScroll;
    private final View chatPage;
    private final View previewPage;
    private final TextView chatTab;
    private final TextView previewTab;
    private final TextView runtimeStatus;
    private final EditText composer;
    private final Button sendButton;
    private final Button stopButton;
    private final ExecutorService chatIoExecutor = Executors.newSingleThreadExecutor();
    private final AtomicInteger drawerLoadGeneration = new AtomicInteger();
    private final AtomicInteger conversationLoadGeneration = new AtomicInteger();
    private Object drawerBackCallback;
    private volatile boolean destroyed;
    private boolean backendReady;
    private boolean turnRunning;
    private boolean preparingChat;
    private boolean conversationBlocked;
    private boolean chatVisible = true;

    AlphaWorkspaceUi(Activity activity, Callbacks callbacks) {
        this.activity = activity;
        this.callbacks = callbacks;
        this.root = new FrameLayout(activity);
        this.root.setBackgroundColor(COLOR_BG);
        installSystemUiInsets();

        LinearLayout main = new LinearLayout(activity);
        main.setOrientation(LinearLayout.VERTICAL);
        main.setBackgroundColor(COLOR_BG);
        root.addView(main, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));

        main.addView(buildAppBar(), new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(60)));

        LinearLayout tabs = new LinearLayout(activity);
        tabs.setOrientation(LinearLayout.HORIZONTAL);
        tabs.setGravity(Gravity.CENTER);
        tabs.setBackgroundColor(COLOR_SURFACE);
        chatTab = tab("◯  Chat", true);
        previewTab = tab("▣  Preview", false);
        tabs.addView(chatTab, weighted(dp(52)));
        tabs.addView(previewTab, weighted(dp(52)));
        main.addView(tabs, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(52)));

        runtimeStatus = new TextView(activity);
        runtimeStatus.setText("Starting AI runtime…");
        runtimeStatus.setTextColor(COLOR_MUTED);
        runtimeStatus.setTextSize(12f);
        runtimeStatus.setGravity(Gravity.CENTER_VERTICAL);
        runtimeStatus.setSingleLine(true);
        runtimeStatus.setEllipsize(android.text.TextUtils.TruncateAt.END);
        runtimeStatus.setPadding(dp(14), 0, dp(14), 0);
        runtimeStatus.setBackgroundColor(COLOR_SURFACE);
        main.addView(runtimeStatus, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(28)));

        FrameLayout content = new FrameLayout(activity);
        LinearLayout.LayoutParams contentParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f);
        contentParams.setMargins(dp(12), dp(10), dp(12), dp(8));
        main.addView(content, contentParams);

        chatMessages = new LinearLayout(activity);
        chatMessages.setOrientation(LinearLayout.VERTICAL);
        chatMessages.setPadding(dp(4), dp(8), dp(4), dp(20));
        chatScroll = new ScrollView(activity);
        chatScroll.setFillViewport(true);
        chatScroll.addView(chatMessages, new ScrollView.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        chatPage = chatScroll;
        content.addView(chatPage, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));

        previewPage = buildPreviewPage();
        previewPage.setVisibility(View.GONE);
        content.addView(previewPage, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));

        LinearLayout composerRow = new LinearLayout(activity);
        composerRow.setOrientation(LinearLayout.HORIZONTAL);
        composerRow.setGravity(Gravity.CENTER_VERTICAL);
        composerRow.setPadding(dp(12), dp(10), dp(12), dp(14));
        composerRow.setBackgroundColor(COLOR_SURFACE);

        composer = new EditText(activity);
        composer.setHint("Type your message…");
        composer.setTextColor(COLOR_TEXT);
        composer.setHintTextColor(COLOR_MUTED);
        composer.setTextSize(15f);
        composer.setSingleLine(false);
        composer.setMaxLines(4);
        composer.setPadding(dp(14), dp(8), dp(14), dp(8));
        composer.setBackground(rounded(COLOR_BG, 14, COLOR_BORDER, 1));
        LinearLayout.LayoutParams composerParams = new LinearLayout.LayoutParams(0, dp(52), 1f);
        composerParams.setMargins(0, 0, dp(8), 0);
        composerRow.addView(composer, composerParams);

        stopButton = actionButton("Stop", false);
        stopButton.setEnabled(false);
        stopButton.setOnClickListener(v -> callbacks.onStopRequested());
        LinearLayout.LayoutParams stopParams = new LinearLayout.LayoutParams(dp(64), dp(46));
        stopParams.setMargins(0, 0, dp(8), 0);
        composerRow.addView(stopButton, stopParams);

        sendButton = actionButton("Send", true);
        sendButton.setOnClickListener(v -> {
            String prompt = composer.getText().toString().trim();
            if (prompt.isEmpty()) return;
            callbacks.onSendRequested(prompt);
        });
        composerRow.addView(sendButton, new LinearLayout.LayoutParams(dp(70), dp(46)));
        main.addView(composerRow, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        drawerList = new LinearLayout(activity);
        drawerList.setOrientation(LinearLayout.VERTICAL);
        drawerOverlay = buildDrawer();
        root.addView(drawerOverlay, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        drawerOverlay.setVisibility(View.GONE);

        chatTab.setOnClickListener(v -> showChat());
        previewTab.setOnClickListener(v -> showPreview());
        showEmptyChat();
        updateComposerEnabled();
    }

    View root() {
        return root;
    }

    private void installSystemUiInsets() {
        // targetSdk 36 is edge-to-edge on Android 15+, so interactive UI must explicitly avoid
        // status/navigation bars and the on-screen keyboard. Keep older Android behavior unchanged
        // because this shell does not opt into edge-to-edge below API 35.
        if (Build.VERSION.SDK_INT < 35) return;
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            Insets systemBars = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            Insets ime = insets.getInsets(WindowInsets.Type.ime());
            view.setPadding(
                    systemBars.left,
                    systemBars.top,
                    systemBars.right,
                    Math.max(systemBars.bottom, ime.bottom));
            return insets;
        });
    }

    void destroy() {
        destroyed = true;
        drawerLoadGeneration.incrementAndGet();
        conversationLoadGeneration.incrementAndGet();
        unregisterDrawerBackCallback();
        chatIoExecutor.shutdownNow();
        updateComposerEnabled();
    }

    private View buildAppBar() {
        LinearLayout bar = new LinearLayout(activity);
        bar.setOrientation(LinearLayout.HORIZONTAL);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setPadding(dp(8), 0, dp(8), 0);
        bar.setBackgroundColor(COLOR_SURFACE);

        TextView menu = iconButton("☰");
        menu.setContentDescription("Open old chats");
        menu.setOnClickListener(v -> openDrawer());
        bar.addView(menu, new LinearLayout.LayoutParams(dp(48), dp(48)));

        TextView logo = new TextView(activity);
        logo.setText("</>");
        logo.setTextColor(COLOR_PRIMARY);
        logo.setTextSize(13f);
        logo.setTypeface(Typeface.MONOSPACE, Typeface.BOLD);
        logo.setGravity(Gravity.CENTER);
        logo.setBackground(rounded(COLOR_PRIMARY_SOFT, 12, 0, 0));
        LinearLayout.LayoutParams logoParams = new LinearLayout.LayoutParams(dp(40), dp(40));
        logoParams.setMargins(dp(2), 0, dp(10), 0);
        bar.addView(logo, logoParams);

        TextView title = new TextView(activity);
        title.setText("VibeCoder");
        title.setTextColor(COLOR_TEXT);
        title.setTextSize(21f);
        title.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        bar.addView(title, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));

        TextView settings = iconButton("⚙");
        settings.setContentDescription("Runtime diagnostics");
        settings.setOnClickListener(v -> callbacks.onOpenDiagnostics());
        bar.addView(settings, new LinearLayout.LayoutParams(dp(48), dp(48)));
        return bar;
    }

    private FrameLayout buildDrawer() {
        FrameLayout overlay = new FrameLayout(activity);
        overlay.setBackgroundColor(Color.argb(72, 17, 24, 39));
        overlay.setOnClickListener(v -> closeDrawer());

        LinearLayout drawer = new LinearLayout(activity);
        drawer.setOrientation(LinearLayout.VERTICAL);
        drawer.setPadding(dp(16), dp(18), dp(16), dp(18));
        drawer.setBackgroundColor(COLOR_SURFACE);
        drawer.setClickable(true);
        drawer.setOnClickListener(v -> { /* consume so taps inside do not close the overlay */ });

        TextView header = new TextView(activity);
        header.setText("Old Chats");
        header.setTextColor(COLOR_MUTED);
        header.setTextSize(17f);
        header.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        header.setPadding(dp(4), dp(6), 0, dp(14));
        drawer.addView(header);

        ScrollView listScroll = new ScrollView(activity);
        listScroll.setFillViewport(true);
        listScroll.addView(drawerList, new ScrollView.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        drawer.addView(listScroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        Button newChat = new Button(activity);
        newChat.setAllCaps(false);
        newChat.setText("＋  New Chat");
        newChat.setTextColor(COLOR_PRIMARY);
        newChat.setTextSize(15f);
        newChat.setBackground(rounded(COLOR_PRIMARY_SOFT, 14, 0, 0));
        newChat.setOnClickListener(v -> {
            if (turnRunning || preparingChat) {
                Toast.makeText(activity, "Finish or stop the current chat action first.", Toast.LENGTH_SHORT).show();
                return;
            }
            conversationLoadGeneration.incrementAndGet();
            callbacks.onConversationSelectionCleared();
            closeDrawer();
            showChat();
            preparingChat = true;
            conversationBlocked = true;
            showCreatingChat();
            updateComposerEnabled();
            callbacks.onNewChatRequested();
        });
        LinearLayout.LayoutParams newChatParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(50));
        newChatParams.setMargins(0, dp(12), 0, 0);
        drawer.addView(newChat, newChatParams);

        int screenWidth = activity.getResources().getDisplayMetrics().widthPixels;
        int drawerWidth = Math.min(dp(320), Math.round(screenWidth * 0.82f));
        FrameLayout.LayoutParams drawerParams = new FrameLayout.LayoutParams(
                drawerWidth, ViewGroup.LayoutParams.MATCH_PARENT, Gravity.START);
        overlay.addView(drawer, drawerParams);
        return overlay;
    }

    private View buildPreviewPage() {
        LinearLayout holder = new LinearLayout(activity);
        holder.setOrientation(LinearLayout.VERTICAL);
        holder.setGravity(Gravity.CENTER);
        holder.setPadding(dp(20), dp(20), dp(20), dp(20));

        LinearLayout card = new LinearLayout(activity);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setGravity(Gravity.CENTER);
        card.setPadding(dp(24), dp(30), dp(24), dp(30));
        card.setBackground(rounded(COLOR_SURFACE, 20, COLOR_BORDER, 1));

        TextView icon = new TextView(activity);
        icon.setText("▣");
        icon.setTextColor(COLOR_PRIMARY);
        icon.setTextSize(44f);
        icon.setGravity(Gravity.CENTER);
        card.addView(icon);

        TextView title = new TextView(activity);
        title.setText("Preview not active yet");
        title.setTextColor(COLOR_TEXT);
        title.setTextSize(19f);
        title.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        title.setGravity(Gravity.CENTER);
        title.setPadding(0, dp(14), 0, dp(8));
        card.addView(title);

        TextView body = new TextView(activity);
        body.setText("The Preview tab is ready. Live local preview will be connected after the preview runtime bridge is implemented.");
        body.setTextColor(COLOR_MUTED);
        body.setTextSize(14f);
        body.setGravity(Gravity.CENTER);
        body.setLineSpacing(0f, 1.15f);
        card.addView(body);

        holder.addView(card, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        return holder;
    }

    private void openDrawer() {
        if (destroyed) return;
        if (turnRunning || preparingChat) {
            Toast.makeText(activity, "Finish or stop the current chat action first.", Toast.LENGTH_SHORT).show();
            return;
        }
        drawerOverlay.setVisibility(View.VISIBLE);
        drawerOverlay.bringToFront();
        registerDrawerBackCallback();
        renderDrawerLoading();
        int generation = drawerLoadGeneration.incrementAndGet();
        try {
            chatIoExecutor.execute(() -> {
                List<ChatEntry> entries = loadPersistedChats();
                activity.runOnUiThread(() -> {
                    if (destroyed || activity.isDestroyed()
                            || generation != drawerLoadGeneration.get()) return;
                    renderDrawerEntries(entries);
                });
            });
        } catch (java.util.concurrent.RejectedExecutionException ignored) {
            // Activity teardown won the race. No UI state should be resurrected.
        }
    }

    private void closeDrawer() {
        drawerLoadGeneration.incrementAndGet();
        drawerOverlay.setVisibility(View.GONE);
        unregisterDrawerBackCallback();
    }

    private void registerDrawerBackCallback() {
        if (Build.VERSION.SDK_INT < 33 || drawerBackCallback != null || destroyed) return;
        drawerBackCallback = Api33Back.register(activity, this::closeDrawer);
    }

    private void unregisterDrawerBackCallback() {
        Object callback = drawerBackCallback;
        drawerBackCallback = null;
        if (Build.VERSION.SDK_INT < 33 || callback == null) return;
        Api33Back.unregister(activity, callback);
    }

    private void showChat() {
        chatVisible = true;
        chatPage.setVisibility(View.VISIBLE);
        previewPage.setVisibility(View.GONE);
        setTabState(chatTab, true);
        setTabState(previewTab, false);
        updateComposerEnabled();
    }

    private void showPreview() {
        chatVisible = false;
        chatPage.setVisibility(View.GONE);
        previewPage.setVisibility(View.VISIBLE);
        setTabState(chatTab, false);
        setTabState(previewTab, true);
        updateComposerEnabled();
    }

    private void showEmptyChat() {
        chatMessages.removeAllViews();
        TextView empty = new TextView(activity);
        empty.setText("Start a new chat\n\nSaved conversations will appear in the Old Chats drawer. Messages are sent to the selected local AI runtime when the status above says AI ready.");
        empty.setTextColor(COLOR_MUTED);
        empty.setTextSize(15f);
        empty.setGravity(Gravity.CENTER);
        empty.setPadding(dp(28), dp(72), dp(28), dp(28));
        empty.setTag("empty_state");
        chatMessages.addView(empty, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
    }

    private void showLoadingChat() {
        chatMessages.removeAllViews();
        TextView loading = new TextView(activity);
        loading.setText("Loading saved chat…");
        loading.setTextColor(COLOR_MUTED);
        loading.setTextSize(15f);
        loading.setGravity(Gravity.CENTER);
        loading.setPadding(dp(28), dp(72), dp(28), dp(28));
        chatMessages.addView(loading, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
    }

    private void showChatLoadFailure() {
        chatMessages.removeAllViews();
        TextView failed = new TextView(activity);
        failed.setText("Saved chat could not be read safely.");
        failed.setTextColor(COLOR_MUTED);
        failed.setTextSize(15f);
        failed.setGravity(Gravity.CENTER);
        failed.setPadding(dp(28), dp(72), dp(28), dp(28));
        chatMessages.addView(failed, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        Toast.makeText(activity, "Saved chat could not be read safely.", Toast.LENGTH_SHORT).show();
    }

    void setBackendState(boolean ready, String status) {
        backendReady = ready;
        runtimeStatus.setText(status == null || status.trim().isEmpty()
                ? (ready ? "AI ready" : "AI runtime unavailable")
                : status.trim());
        runtimeStatus.setTextColor(ready ? COLOR_PRIMARY : COLOR_MUTED);
        updateComposerEnabled();
    }

    void setTurnRunning(boolean running) {
        turnRunning = running;
        updateComposerEnabled();
    }

    void showNewChatReady() {
        preparingChat = false;
        conversationBlocked = false;
        conversationLoadGeneration.incrementAndGet();
        showChat();
        showEmptyChat();
        updateComposerEnabled();
        composer.requestFocus();
    }

    void showCreateChatFailure(String message) {
        preparingChat = false;
        conversationBlocked = false;
        showChat();
        chatMessages.removeAllViews();
        addSystemNotice(message == null ? "New chat could not be created." : message);
        updateComposerEnabled();
    }

    void appendUserMessage(String text) {
        if (text == null || text.isEmpty()) return;
        removeEmptyStateIfPresent();
        addMessageBubble("user", truncateUtf8ForDisplay(text, MAX_RENDERED_SINGLE_MESSAGE_BYTES));
        composer.setText("");
        scrollChatToBottom();
    }

    void appendAssistantMessage(String text) {
        if (text == null || text.isEmpty()) return;
        removeEmptyStateIfPresent();
        addMessageBubble("assistant", truncateUtf8ForDisplay(text, MAX_RENDERED_SINGLE_MESSAGE_BYTES));
        scrollChatToBottom();
    }

    void showTurnNotice(String message) {
        addSystemNotice(message == null ? "Turn failed." : message);
        scrollChatToBottom();
    }

    private void showCreatingChat() {
        chatMessages.removeAllViews();
        TextView loading = new TextView(activity);
        loading.setText("Creating local chat…");
        loading.setTextColor(COLOR_MUTED);
        loading.setTextSize(15f);
        loading.setGravity(Gravity.CENTER);
        loading.setPadding(dp(28), dp(72), dp(28), dp(28));
        loading.setTag("empty_state");
        chatMessages.addView(loading, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
    }

    private void updateComposerEnabled() {
        boolean canCompose = backendReady && chatVisible && !turnRunning && !preparingChat
                && !conversationBlocked && !destroyed;
        composer.setEnabled(canCompose);
        sendButton.setEnabled(canCompose);
        stopButton.setEnabled(backendReady && chatVisible && turnRunning && !destroyed);
    }

    private void removeEmptyStateIfPresent() {
        if (chatMessages.getChildCount() == 1) {
            View only = chatMessages.getChildAt(0);
            if ("empty_state".equals(only.getTag())) chatMessages.removeAllViews();
        }
    }

    private void addSystemNotice(String message) {
        TextView notice = new TextView(activity);
        notice.setText(message);
        notice.setTextColor(COLOR_MUTED);
        notice.setTextSize(12f);
        notice.setGravity(Gravity.CENTER);
        notice.setPadding(dp(12), dp(10), dp(12), dp(10));
        notice.setBackground(rounded(COLOR_BG, 10, COLOR_BORDER, 1));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        params.setMargins(0, dp(6), 0, dp(6));
        chatMessages.addView(notice, params);
    }

    private void scrollChatToBottom() {
        chatScroll.post(() -> {
            if (!destroyed) chatScroll.fullScroll(View.FOCUS_DOWN);
        });
    }

    private void renderDrawerLoading() {
        drawerList.removeAllViews();
        TextView loading = new TextView(activity);
        loading.setText("Loading chats…");
        loading.setTextColor(COLOR_MUTED);
        loading.setTextSize(14f);
        loading.setPadding(dp(4), dp(18), dp(4), dp(18));
        drawerList.addView(loading);
    }

    private void renderDrawerEntries(List<ChatEntry> entries) {
        drawerList.removeAllViews();
        if (entries.isEmpty()) {
            TextView empty = new TextView(activity);
            empty.setText("No saved chats yet");
            empty.setTextColor(COLOR_MUTED);
            empty.setTextSize(14f);
            empty.setPadding(dp(4), dp(18), dp(4), dp(18));
            drawerList.addView(empty);
            return;
        }
        for (ChatEntry entry : entries) {
            TextView item = new TextView(activity);
            item.setText("◯  " + entry.title);
            item.setTextColor(COLOR_TEXT);
            item.setTextSize(15f);
            item.setGravity(Gravity.CENTER_VERTICAL);
            item.setPadding(dp(14), dp(8), dp(10), dp(8));
            item.setSingleLine(true);
            item.setEllipsize(android.text.TextUtils.TruncateAt.END);
            item.setBackground(rounded(COLOR_BG, 12, COLOR_BORDER, 1));
            item.setOnClickListener(v -> loadConversationAsync(entry.file));
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, dp(52));
            params.setMargins(0, 0, 0, dp(8));
            drawerList.addView(item, params);
        }
    }

    private List<ChatEntry> loadPersistedChats() {
        List<ChatEntry> entries = new ArrayList<>();
        try {
            File rootDir = conversationRoot();
            if (!rootDir.isDirectory() || isSymlink(rootDir)) return entries;
            File canonicalRoot = rootDir.getCanonicalFile();
            File[] files = canonicalRoot.listFiles();
            if (files == null) return entries;

            List<File> candidates = new ArrayList<>();
            for (File file : files) {
                if (isSafeConversationFile(canonicalRoot, file)) candidates.add(file);
            }
            candidates.sort(Comparator.comparingLong(File::lastModified).reversed());
            for (File file : candidates) {
                if (entries.size() >= MAX_CHAT_FILES) break;
                JSONObject json = readConversationJson(file);
                if (json == null) continue;
                entries.add(new ChatEntry(file, safeChatTitle(json), file.lastModified()));
            }
        } catch (Throwable ignored) {
            // Drawer stays empty rather than bypassing the Rust store's authority or showing stale data.
        }
        return entries;
    }

    private void loadConversationAsync(File file) {
        if (destroyed) return;
        callbacks.onConversationSelectionCleared();
        closeDrawer();
        showChat();
        preparingChat = true;
        conversationBlocked = true;
        showLoadingChat();
        updateComposerEnabled();
        int generation = conversationLoadGeneration.incrementAndGet();
        try {
            chatIoExecutor.execute(() -> {
                JSONObject json = readConversationJson(file);
                activity.runOnUiThread(() -> {
                    if (destroyed || activity.isDestroyed()
                            || generation != conversationLoadGeneration.get()) return;
                    if (json == null) {
                        preparingChat = false;
                        conversationBlocked = true;
                        showChatLoadFailure();
                        updateComposerEnabled();
                        return;
                    }
                    renderConversation(json);
                    boolean recoveryBlocked = json.optBoolean("turn_pending", false)
                            || json.optBoolean("session_creation_pending", false);
                    conversationBlocked = recoveryBlocked;
                    if (recoveryBlocked) {
                        addSystemNotice("This saved chat needs recovery before another message can be sent.");
                    } else {
                        callbacks.onConversationSelected(
                                json.optString("project_id", ""),
                                json.optString("conversation_id", ""));
                    }
                    preparingChat = false;
                    updateComposerEnabled();
                });
            });
        } catch (java.util.concurrent.RejectedExecutionException ignored) {
            // Activity teardown won the race. No failed/stale conversation should render.
        }
    }

    private void renderConversation(JSONObject json) {
        JSONArray messages = json.optJSONArray("messages");
        if (messages == null) {
            showChatLoadFailure();
            return;
        }

        List<RenderedMessage> rendered = new ArrayList<>();
        int earliestByCount = Math.max(0, messages.length() - MAX_RENDERED_MESSAGES);
        int remainingBytes = MAX_RENDERED_TEXT_BYTES;
        boolean displayOmitted = earliestByCount > 0;
        for (int index = messages.length() - 1; index >= earliestByCount; index--) {
            JSONObject message = messages.optJSONObject(index);
            if (message == null) continue;
            String role = message.optString("role", "");
            String text = message.optString("text", "");
            if (text.isEmpty() || (!"user".equals(role) && !"assistant".equals(role))) continue;
            if (remainingBytes <= 0) {
                displayOmitted = true;
                break;
            }
            int perMessageLimit = Math.min(MAX_RENDERED_SINGLE_MESSAGE_BYTES, remainingBytes);
            String displayText = truncateUtf8ForDisplay(text, perMessageLimit);
            int displayBytes = utf8Length(displayText);
            if (!displayText.equals(text)) displayOmitted = true;
            rendered.add(0, new RenderedMessage(role, displayText));
            remainingBytes -= displayBytes;
        }

        chatMessages.removeAllViews();
        if (displayOmitted) addDisplayLimitNotice();
        for (RenderedMessage message : rendered) {
            addMessageBubble(message.role, message.text);
        }
        if (rendered.isEmpty()) showEmptyChat();
        chatScroll.post(() -> {
            if (!destroyed) chatScroll.fullScroll(View.FOCUS_DOWN);
        });
    }

    private void addDisplayLimitNotice() {
        TextView notice = new TextView(activity);
        notice.setText("Large/older chat content is hidden in this Alpha view. Persisted history was not modified.");
        notice.setTextColor(COLOR_MUTED);
        notice.setTextSize(12f);
        notice.setGravity(Gravity.CENTER);
        notice.setPadding(dp(12), dp(8), dp(12), dp(8));
        notice.setBackground(rounded(COLOR_BG, 10, COLOR_BORDER, 1));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        params.setMargins(0, 0, 0, dp(8));
        chatMessages.addView(notice, params);
    }

    private File conversationRoot() {
        return new File(activity.getFilesDir(), "vibecoder/state/conversations");
    }

    private JSONObject readConversationJson(File file) {
        try {
            File canonicalRoot = conversationRoot().getCanonicalFile();
            if (!isSafeConversationFile(canonicalRoot, file)) return null;
            long length = file.length();
            if (length <= 0 || length > MAX_CHAT_FILE_BYTES) return null;
            JSONObject json;
            try (InputStream input = new FileInputStream(file);
                 ByteArrayOutputStream output = new ByteArrayOutputStream((int) Math.min(length, 64 * 1024))) {
                byte[] buffer = new byte[4096];
                int read;
                while ((read = input.read(buffer)) != -1) {
                    if (output.size() + read > MAX_CHAT_FILE_BYTES) return null;
                    output.write(buffer, 0, read);
                }
                json = new JSONObject(output.toString(StandardCharsets.UTF_8.name()));
            }
            return conversationJsonIsValidForDisplay(file, json) ? json : null;
        } catch (Throwable ignored) {
            return null;
        }
    }

    private boolean conversationJsonIsValidForDisplay(File file, JSONObject json) {
        try {
            if (json.optInt("schema", -1) != 1) return false;
            String name = file.getName();
            String stem = name.substring(0, name.length() - ".json".length());
            int separator = stem.indexOf("--");
            if (separator <= 0 || separator != stem.lastIndexOf("--")) return false;
            String projectText = stem.substring(0, separator);
            String conversationText = stem.substring(separator + 2);
            UUID projectId = UUID.fromString(projectText);
            UUID conversationId = UUID.fromString(conversationText);
            if (!projectText.equals(projectId.toString())
                    || !conversationText.equals(conversationId.toString())
                    || !projectText.equals(json.optString("project_id", ""))
                    || !conversationText.equals(json.optString("conversation_id", ""))) {
                return false;
            }

            String title = json.optString("title", "");
            if (!title.isEmpty()) {
                if (!title.equals(title.trim())
                        || utf8Length(title) > MAX_CONVERSATION_TITLE_BYTES
                        || containsControl(title)) return false;
            }

            JSONArray messages = json.optJSONArray("messages");
            if (messages == null || messages.length() > MAX_CONVERSATION_MESSAGES) return false;
            long totalTextBytes = 0;
            String lastRole = "";
            for (int index = 0; index < messages.length(); index++) {
                JSONObject message = messages.optJSONObject(index);
                if (message == null || message.optLong("sequence", -1L) != index) return false;
                String role = message.optString("role", "");
                String text = message.optString("text", "");
                if ((!"user".equals(role) && !"assistant".equals(role))
                        || text.isEmpty() || text.indexOf('\0') >= 0) return false;
                int textBytes = utf8Length(text);
                if (textBytes > MAX_CONVERSATION_MESSAGE_BYTES) return false;
                totalTextBytes += textBytes;
                if (totalTextBytes > MAX_CONVERSATION_TEXT_BYTES) return false;
                lastRole = role;
            }
            boolean turnPending = json.optBoolean("turn_pending", false);
            boolean sessionCreationPending = json.optBoolean("session_creation_pending", false);
            Object agentSession = json.opt("agent_session");
            boolean agentSessionMissing = agentSession == null || agentSession == JSONObject.NULL;
            if (sessionCreationPending != agentSessionMissing) return false;
            if (turnPending && !"user".equals(lastRole)) return false;
            if (sessionCreationPending && (turnPending || messages.length() != 0)) return false;
            return true;
        } catch (Throwable ignored) {
            return false;
        }
    }

    private boolean isSafeConversationFile(File canonicalRoot, File file) {
        try {
            if (file == null || !file.getName().endsWith(".json") || !file.isFile()) return false;
            if (isSymlink(file)) return false;
            File canonical = file.getCanonicalFile();
            if (canonical.getParentFile() == null || !canonical.getParentFile().equals(canonicalRoot)) {
                return false;
            }
            String name = canonical.getName();
            String stem = name.substring(0, name.length() - ".json".length());
            int separator = stem.indexOf("--");
            if (separator <= 0 || separator != stem.lastIndexOf("--")) return false;
            String projectText = stem.substring(0, separator);
            String conversationText = stem.substring(separator + 2);
            UUID project = UUID.fromString(projectText);
            UUID conversation = UUID.fromString(conversationText);
            return projectText.equals(project.toString()) && conversationText.equals(conversation.toString());
        } catch (Throwable ignored) {
            return false;
        }
    }

    private static boolean isSymlink(File file) {
        try {
            return Build.VERSION.SDK_INT >= 26 && Files.isSymbolicLink(file.toPath());
        } catch (Throwable ignored) {
            return true;
        }
    }

    private static int utf8Length(String text) {
        return text.getBytes(StandardCharsets.UTF_8).length;
    }

    private static boolean containsControl(String text) {
        for (int offset = 0; offset < text.length();) {
            int codePoint = text.codePointAt(offset);
            if (Character.isISOControl(codePoint)) return true;
            offset += Character.charCount(codePoint);
        }
        return false;
    }

    private static String safeChatTitle(JSONObject json) {
        String title = json.optString("title", "").trim();
        if (!title.isEmpty()) return truncate(title, 34);
        JSONArray messages = json.optJSONArray("messages");
        if (messages != null) {
            for (int i = 0; i < messages.length(); i++) {
                JSONObject message = messages.optJSONObject(i);
                if (message != null && "user".equals(message.optString("role", ""))) {
                    String text = message.optString("text", "").trim().replace('\n', ' ');
                    if (!text.isEmpty()) return truncate(text, 34);
                }
            }
        }
        return "Untitled chat";
    }

    private static String truncate(String text, int maxCodePoints) {
        int count = text.codePointCount(0, text.length());
        if (count <= maxCodePoints) return text;
        int end = text.offsetByCodePoints(0, Math.max(1, maxCodePoints - 1));
        return text.substring(0, end) + "…";
    }

    private static String truncateUtf8ForDisplay(String text, int maxBytes) {
        if (utf8Length(text) <= maxBytes) return text;
        final String suffix = "\n\n[Message truncated in Alpha UI]";
        int suffixBytes = utf8Length(suffix);
        int contentBudget = Math.max(0, maxBytes - suffixBytes);
        int used = 0;
        int offset = 0;
        while (offset < text.length()) {
            int codePoint = text.codePointAt(offset);
            int codePointBytes;
            if (codePoint <= 0x7f) codePointBytes = 1;
            else if (codePoint <= 0x7ff) codePointBytes = 2;
            else if (codePoint <= 0xffff) codePointBytes = 3;
            else codePointBytes = 4;
            if (used + codePointBytes > contentBudget) break;
            used += codePointBytes;
            offset += Character.charCount(codePoint);
        }
        return text.substring(0, offset) + suffix;
    }

    private void addMessageBubble(String role, String text) {
        boolean user = "user".equals(role);
        LinearLayout row = new LinearLayout(activity);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(user ? Gravity.END : Gravity.START);
        row.setPadding(user ? dp(36) : 0, dp(6), user ? 0 : dp(36), dp(6));

        TextView bubble = new TextView(activity);
        bubble.setText(text);
        bubble.setTextColor(COLOR_TEXT);
        bubble.setTextSize(15f);
        bubble.setLineSpacing(0f, 1.12f);
        bubble.setPadding(dp(14), dp(12), dp(14), dp(12));
        bubble.setBackground(rounded(user ? COLOR_PRIMARY_SOFT : COLOR_ASSISTANT, 16, COLOR_BORDER, 1));
        row.addView(bubble, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        chatMessages.addView(row, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
    }

    private TextView tab(String text, boolean active) {
        TextView tab = new TextView(activity);
        tab.setText(text);
        tab.setTextSize(16f);
        tab.setGravity(Gravity.CENTER);
        setTabState(tab, active);
        return tab;
    }

    private void setTabState(TextView tab, boolean active) {
        tab.setTextColor(active ? COLOR_PRIMARY : COLOR_MUTED);
        tab.setTypeface(Typeface.DEFAULT, active ? Typeface.BOLD : Typeface.NORMAL);
        tab.setBackground(active
                ? rounded(COLOR_PRIMARY_SOFT, 0, 0, 0)
                : rounded(COLOR_SURFACE, 0, 0, 0));
    }

    private TextView iconButton(String glyph) {
        TextView view = new TextView(activity);
        view.setText(glyph);
        view.setTextColor(COLOR_TEXT);
        view.setTextSize(23f);
        view.setGravity(Gravity.CENTER);
        view.setClickable(true);
        view.setFocusable(true);
        return view;
    }

    private Button actionButton(String text, boolean primary) {
        Button button = new Button(activity);
        button.setAllCaps(false);
        button.setText(text);
        button.setTextSize(14f);
        button.setTextColor(primary ? Color.WHITE : COLOR_TEXT);
        button.setBackground(primary
                ? rounded(COLOR_PRIMARY, 13, 0, 0)
                : rounded(COLOR_SURFACE, 13, COLOR_BORDER, 1));
        button.setPadding(0, 0, 0, 0);
        return button;
    }

    private LinearLayout.LayoutParams weighted(int height) {
        return new LinearLayout.LayoutParams(0, height, 1f);
    }

    private GradientDrawable rounded(int fillColor, int radiusDp, int strokeColor, int strokeWidthDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fillColor);
        drawable.setCornerRadius(dp(radiusDp));
        if (strokeWidthDp > 0) drawable.setStroke(dp(strokeWidthDp), strokeColor);
        return drawable;
    }

    private int dp(int value) {
        return Math.round(value * activity.getResources().getDisplayMetrics().density);
    }

    @android.annotation.TargetApi(33)
    private static final class Api33Back {
        private Api33Back() {}

        static Object register(Activity activity, Runnable action) {
            android.window.OnBackInvokedCallback callback = action::run;
            activity.getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    android.window.OnBackInvokedDispatcher.PRIORITY_OVERLAY, callback);
            return callback;
        }

        static void unregister(Activity activity, Object callback) {
            activity.getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(
                    (android.window.OnBackInvokedCallback) callback);
        }
    }

    private static final class RenderedMessage {
        final String role;
        final String text;

        RenderedMessage(String role, String text) {
            this.role = role;
            this.text = text;
        }
    }

    private static final class ChatEntry {
        final File file;
        final String title;
        final long modifiedMs;

        ChatEntry(File file, String title, long modifiedMs) {
            this.file = file;
            this.title = title;
            this.modifiedMs = modifiedMs;
        }
    }
}
