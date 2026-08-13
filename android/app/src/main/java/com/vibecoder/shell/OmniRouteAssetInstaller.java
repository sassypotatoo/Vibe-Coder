package com.vibecoder.shell;

import android.content.res.AssetManager;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.io.InputStream;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

/**
 * Installs the sealed OmniRoute APK asset into app-private storage.
 *
 * The APK asset is immutable input. Runtime execution never happens from the APK asset tree and
 * never happens from external/shared storage. Every payload file is copied to a fresh staging
 * directory, checked against the sealed bundle manifest, and only then atomically promoted.
 */
final class OmniRouteAssetInstaller {
    static final String ASSET_ROOT = "omniroute/bundle";
    static final String MANIFEST_NAME = ".vibecoder-omniroute-bundle.json";
    static final String RECEIPT_NAME = ".vibecoder-omniroute-install.json";
    static final String INSTALL_RELATIVE_ROOT = "vibecoder/runtime/omniroute";

    private static final String EXPECTED_COMPONENT = "omniroute";
    private static final String EXPECTED_VERSION = "3.8.50";
    private static final String EXPECTED_PROFILE = "vibecoder-omniroute-android-backend-v1";
    private static final String EXPECTED_NODE = "24.19.0";
    private static final String EXPECTED_SOURCE_SHA256 =
            "1c33cd369119f17cc8343e7373254f7a93623166dc123246119c379ea9a17ad7";
    private static final String EXPECTED_PATCH_SHA256 =
            "aec0f63fb0dec08f24fffde9209504ec447e9428bec1cd64c033649ed275fe3d";

    private static final int MAX_MANIFEST_BYTES = 8 * 1024 * 1024;
    private static final int MAX_RECEIPT_BYTES = 128 * 1024;
    private static final int MAX_FILES = 100_000;
    private static final long MAX_FILE_BYTES = 256L * 1024L * 1024L;
    private static final long MAX_TOTAL_BYTES = 1024L * 1024L * 1024L;
    private static final int COPY_BUFFER_BYTES = 128 * 1024;

    private OmniRouteAssetInstaller() {}

    static Result ensureInstalled(AssetManager assets, File filesDir) throws Exception {
        byte[] packagedManifestBytes;
        try {
            packagedManifestBytes = readAssetBounded(
                    assets,
                    ASSET_ROOT + "/" + MANIFEST_NAME,
                    MAX_MANIFEST_BYTES);
        } catch (FileNotFoundException missing) {
            return Result.notPackaged();
        }

        BundleManifest packaged = BundleManifest.parse(packagedManifestBytes);
        String packagedManifestSha = sha256(packagedManifestBytes);

        File runtimeRoot = canonicalChild(filesDir, "vibecoder/runtime");
        ensurePrivateDirectory(runtimeRoot);
        File lockFile = canonicalChild(runtimeRoot, ".omniroute-install.lock");
        if (!lockFile.exists() && !lockFile.createNewFile()) {
            throw new IOException("omniroute_install_lock_create_failed");
        }
        setOwnerOnly(lockFile, false);

        try (FileOutputStream lockStream = new FileOutputStream(lockFile, true);
             FileChannel lockChannel = lockStream.getChannel();
             FileLock installLock = lockChannel.lock()) {
            if (!installLock.isValid()) {
                throw new IOException("omniroute_install_lock_invalid");
            }
            return ensureInstalledLocked(assets, runtimeRoot, packaged, packagedManifestBytes, packagedManifestSha);
        }
    }

    private static Result ensureInstalledLocked(
            AssetManager assets,
            File runtimeRoot,
            BundleManifest packaged,
            byte[] packagedManifestBytes,
            String packagedManifestSha) throws Exception {
        File current = canonicalChild(runtimeRoot, "omniroute");
        File previous = canonicalChild(runtimeRoot, ".omniroute-previous");
        cleanupStaleStages(runtimeRoot);
        recoverPreviousIfNeeded(current, previous);

        Verification currentVerification = verifyInstalled(current, packagedManifestSha, true);
        if (currentVerification.valid) {
            deleteTree(previous);
            return Result.reused(current, packaged, packagedManifestSha);
        }

        File stage = canonicalChild(runtimeRoot, ".omniroute-stage-" + packaged.treeSha256.substring(0, 16));
        deleteTree(stage);
        ensurePrivateDirectory(stage);

        boolean committed = false;
        try {
            for (ManifestFile item : packaged.files) {
                File target = canonicalRelativeFile(stage, item.path);
                File parent = target.getParentFile();
                if (parent == null) throw new IOException("omniroute_asset_parent_missing");
                ensurePrivateDirectory(parent);
                copyAndVerifyAsset(assets, ASSET_ROOT + "/" + item.path, target, item);
            }

            writeAtomic(canonicalChild(stage, MANIFEST_NAME), packagedManifestBytes, MAX_MANIFEST_BYTES);
            byte[] receipt = buildReceipt(packaged, packagedManifestSha);
            writeAtomic(canonicalChild(stage, RECEIPT_NAME), receipt, MAX_RECEIPT_BYTES);

            Verification staged = verifyInstalled(stage, packagedManifestSha, true);
            if (!staged.valid) {
                throw new IOException("omniroute_stage_verification_failed:" + staged.reason);
            }

            deleteTree(previous);
            if (current.exists()) {
                moveSameFilesystem(current, previous);
            }
            try {
                moveSameFilesystem(stage, current);
                committed = true;
            } catch (Throwable commitError) {
                if (!current.exists() && previous.exists()) {
                    moveSameFilesystem(previous, current);
                }
                throw commitError;
            }

            Verification installed = verifyInstalled(current, packagedManifestSha, true);
            if (!installed.valid) {
                deleteTree(current);
                if (previous.exists()) {
                    moveSameFilesystem(previous, current);
                }
                throw new IOException("omniroute_post_commit_verification_failed:" + installed.reason);
            }
            deleteTree(previous);
            return Result.installed(current, packaged, packagedManifestSha);
        } finally {
            if (!committed) deleteTree(stage);
        }
    }

    private static void recoverPreviousIfNeeded(File current, File previous) throws Exception {
        if (!previous.exists()) return;
        Verification currentAny = verifyInstalled(current, null, false);
        if (currentAny.valid) {
            deleteTree(previous);
            return;
        }
        Verification previousAny = verifyInstalled(previous, null, false);
        if (!previousAny.valid) {
            deleteTree(previous);
            return;
        }
        deleteTree(current);
        moveSameFilesystem(previous, current);
    }

    private static Verification verifyInstalled(File root, String expectedManifestSha, boolean requireReceipt) {
        try {
            if (!root.isDirectory() || Files.isSymbolicLink(root.toPath())) {
                return Verification.fail("root_missing_or_symlink");
            }
            File manifestFile = canonicalChild(root, MANIFEST_NAME);
            byte[] manifestBytes = readFileBounded(manifestFile, MAX_MANIFEST_BYTES);
            String manifestSha = sha256(manifestBytes);
            if (expectedManifestSha != null && !constantEquals(manifestSha, expectedManifestSha)) {
                return Verification.fail("manifest_sha_mismatch");
            }
            BundleManifest manifest = BundleManifest.parse(manifestBytes);

            if (requireReceipt) {
                File receiptFile = canonicalChild(root, RECEIPT_NAME);
                byte[] receiptBytes = readFileBounded(receiptFile, MAX_RECEIPT_BYTES);
                JSONObject receipt = new JSONObject(new String(receiptBytes, StandardCharsets.UTF_8));
                if (receipt.optInt("schema", -1) != 1
                        || !EXPECTED_COMPONENT.equals(receipt.optString("component_id"))
                        || !EXPECTED_VERSION.equals(receipt.optString("version"))
                        || !EXPECTED_PROFILE.equals(receipt.optString("profile_id"))
                        || !constantEquals(manifestSha, receipt.optString("manifest_sha256"))
                        || !constantEquals(manifest.treeSha256, receipt.optString("tree_sha256"))) {
                    return Verification.fail("receipt_mismatch");
                }
            }

            Set<String> expectedPaths = new HashSet<>();
            expectedPaths.add(MANIFEST_NAME);
            if (requireReceipt) expectedPaths.add(RECEIPT_NAME);
            long totalBytes = 0;
            for (ManifestFile item : manifest.files) {
                expectedPaths.add(item.path);
                File file = canonicalRelativeFile(root, item.path);
                if (!file.isFile() || Files.isSymbolicLink(file.toPath())) {
                    return Verification.fail("payload_missing_or_symlink:" + item.path);
                }
                long size = file.length();
                if (size != item.size) return Verification.fail("payload_size_mismatch:" + item.path);
                String actual = sha256File(file);
                if (!constantEquals(actual, item.sha256)) {
                    return Verification.fail("payload_sha_mismatch:" + item.path);
                }
                totalBytes = checkedAdd(totalBytes, size);
            }
            if (totalBytes != manifest.totalBytes) return Verification.fail("total_bytes_mismatch");

            Set<String> actualPaths = new HashSet<>();
            try (java.util.stream.Stream<Path> stream = Files.walk(root.toPath())) {
                stream.forEach(path -> {
                    if (path.equals(root.toPath())) return;
                    try {
                        if (Files.isSymbolicLink(path)) {
                            throw new RuntimeException("omniroute_install_symlink_forbidden:" + root.toPath().relativize(path));
                        }
                        if (Files.isRegularFile(path)) {
                            actualPaths.add(root.toPath().relativize(path).toString().replace(File.separatorChar, '/'));
                        }
                    } catch (RuntimeException runtime) {
                        throw runtime;
                    }
                });
            }
            if (!actualPaths.equals(expectedPaths)) return Verification.fail("unexpected_or_missing_files");
            return Verification.ok();
        } catch (Throwable error) {
            return Verification.fail(safeReason(error));
        }
    }

    private static void copyAndVerifyAsset(
            AssetManager assets,
            String assetPath,
            File target,
            ManifestFile expected) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        long written = 0;
        try (InputStream input = assets.open(assetPath, AssetManager.ACCESS_STREAMING);
             FileOutputStream output = new FileOutputStream(target, false)) {
            byte[] buffer = new byte[COPY_BUFFER_BYTES];
            int read;
            while ((read = input.read(buffer)) != -1) {
                written = checkedAdd(written, read);
                if (written > expected.size || written > MAX_FILE_BYTES) {
                    throw new IOException("omniroute_asset_size_exceeded:" + expected.path);
                }
                digest.update(buffer, 0, read);
                output.write(buffer, 0, read);
            }
            output.flush();
            output.getFD().sync();
        }
        setOwnerOnly(target, false);
        if (written != expected.size) throw new IOException("omniroute_asset_size_mismatch:" + expected.path);
        String actual = hex(digest.digest());
        if (!constantEquals(actual, expected.sha256)) {
            throw new IOException("omniroute_asset_sha_mismatch:" + expected.path);
        }
    }

    private static byte[] buildReceipt(BundleManifest manifest, String manifestSha) throws Exception {
        JSONObject receipt = new JSONObject();
        receipt.put("schema", 1);
        receipt.put("component_id", EXPECTED_COMPONENT);
        receipt.put("version", EXPECTED_VERSION);
        receipt.put("profile_id", EXPECTED_PROFILE);
        receipt.put("manifest_sha256", manifestSha);
        receipt.put("tree_sha256", manifest.treeSha256);
        receipt.put("install_root", INSTALL_RELATIVE_ROOT);
        receipt.put("source", "verified_apk_asset");
        receipt.put("service_round_trip_proven", false);
        return (receipt.toString() + "\n").getBytes(StandardCharsets.UTF_8);
    }

    private static void cleanupStaleStages(File runtimeRoot) throws Exception {
        File[] children = runtimeRoot.listFiles();
        if (children == null) return;
        for (File child : children) {
            if (child.getName().startsWith(".omniroute-stage-")) deleteTree(child);
        }
    }

    private static void moveSameFilesystem(File source, File target) throws Exception {
        if (!source.exists()) throw new IOException("omniroute_move_source_missing");
        if (target.exists()) throw new IOException("omniroute_move_target_exists");
        try {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException ignored) {
            Files.move(source.toPath(), target.toPath());
        }
    }

    private static void writeAtomic(File target, byte[] bytes, int maxBytes) throws Exception {
        if (bytes.length > maxBytes) throw new IOException("omniroute_atomic_write_too_large");
        File parent = target.getParentFile();
        if (parent == null) throw new IOException("omniroute_atomic_write_parent_missing");
        ensurePrivateDirectory(parent);
        File temp = canonicalChild(parent, "." + target.getName() + ".tmp");
        if (Files.isSymbolicLink(temp.toPath())) throw new IOException("omniroute_atomic_temp_symlink_forbidden");
        try (FileOutputStream output = new FileOutputStream(temp, false)) {
            output.write(bytes);
            output.flush();
            output.getFD().sync();
        }
        setOwnerOnly(temp, false);
        try {
            Files.move(temp.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException ignored) {
            Files.move(temp.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static void ensurePrivateDirectory(File directory) throws Exception {
        if (directory.exists()) {
            if (!directory.isDirectory() || Files.isSymbolicLink(directory.toPath())) {
                throw new IOException("omniroute_private_directory_invalid:" + directory.getName());
            }
        } else if (!directory.mkdirs() && !directory.isDirectory()) {
            throw new IOException("omniroute_private_directory_create_failed:" + directory.getName());
        }
        setOwnerOnly(directory, true);
    }

    private static void setOwnerOnly(File file, boolean directory) {
        // Android's app-private data directory is already UID-confined. Tighten ordinary mode bits too.
        file.setReadable(false, false);
        file.setWritable(false, false);
        file.setExecutable(false, false);
        file.setReadable(true, true);
        file.setWritable(true, true);
        if (directory) file.setExecutable(true, true);
    }

    private static File canonicalChild(File root, String relative) throws Exception {
        return canonicalRelativeFile(root, relative);
    }

    private static File canonicalRelativeFile(File root, String relative) throws Exception {
        validateRelativePath(relative);
        File canonicalRoot = root.getCanonicalFile();
        File candidate = new File(canonicalRoot, relative).getCanonicalFile();
        String rootPath = canonicalRoot.getPath();
        String candidatePath = candidate.getPath();
        if (!candidatePath.equals(rootPath) && !candidatePath.startsWith(rootPath + File.separator)) {
            throw new IOException("omniroute_path_escape:" + relative);
        }
        return candidate;
    }

    private static void validateRelativePath(String value) throws IOException {
        if (value == null || value.isEmpty() || value.length() > 4096) {
            throw new IOException("omniroute_relative_path_invalid");
        }
        if (value.startsWith("/") || value.startsWith("\\") || value.contains("\\") || value.indexOf('\0') >= 0) {
            throw new IOException("omniroute_relative_path_invalid:" + value);
        }
        String[] parts = value.split("/", -1);
        for (String part : parts) {
            if (part.isEmpty() || ".".equals(part) || "..".equals(part)) {
                throw new IOException("omniroute_relative_path_invalid:" + value);
            }
        }
    }

    private static void deleteTree(File root) throws Exception {
        if (!root.exists()) return;
        Path path = root.toPath();
        if (Files.isSymbolicLink(path)) {
            Files.deleteIfExists(path);
            return;
        }
        try (java.util.stream.Stream<Path> stream = Files.walk(path)) {
            List<Path> paths = new ArrayList<>();
            stream.forEach(paths::add);
            Collections.sort(paths, Collections.reverseOrder());
            for (Path item : paths) Files.deleteIfExists(item);
        }
    }

    private static byte[] readAssetBounded(AssetManager assets, String path, int maxBytes) throws Exception {
        try (InputStream input = assets.open(path, AssetManager.ACCESS_STREAMING)) {
            return readBounded(input, maxBytes);
        }
    }

    private static byte[] readFileBounded(File file, int maxBytes) throws Exception {
        if (!file.isFile() || file.length() > maxBytes) throw new IOException("omniroute_file_missing_or_too_large");
        try (InputStream input = new FileInputStream(file)) {
            return readBounded(input, maxBytes);
        }
    }

    private static byte[] readBounded(InputStream input, int maxBytes) throws Exception {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        byte[] buffer = new byte[32 * 1024];
        int read;
        while ((read = input.read(buffer)) != -1) {
            if (output.size() + read > maxBytes) throw new IOException("omniroute_bounded_read_exceeded");
            output.write(buffer, 0, read);
        }
        return output.toByteArray();
    }

    private static long checkedAdd(long left, long right) throws IOException {
        if (right < 0 || left > Long.MAX_VALUE - right) throw new IOException("omniroute_size_overflow");
        return left + right;
    }

    private static String sha256(byte[] bytes) throws Exception {
        return hex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }

    private static String sha256File(File file) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (InputStream input = new FileInputStream(file)) {
            byte[] buffer = new byte[128 * 1024];
            int read;
            while ((read = input.read(buffer)) != -1) digest.update(buffer, 0, read);
        }
        return hex(digest.digest());
    }

    private static String hex(byte[] bytes) {
        StringBuilder out = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) out.append(String.format(Locale.ROOT, "%02x", value & 0xff));
        return out.toString();
    }

    private static boolean constantEquals(String left, String right) {
        if (left == null || right == null) return false;
        return MessageDigest.isEqual(
                left.getBytes(StandardCharsets.US_ASCII),
                right.getBytes(StandardCharsets.US_ASCII));
    }

    private static String safeReason(Throwable error) {
        String message = error.getMessage();
        String raw = error.getClass().getSimpleName() + ":" + (message == null ? "no_message" : message);
        return raw.length() > 300 ? raw.substring(0, 300) : raw;
    }

    static final class Result {
        final boolean packaged;
        final boolean verified;
        final boolean installedNow;
        final String status;
        final String installRoot;
        final String treeSha256;
        final String manifestSha256;

        private Result(
                boolean packaged,
                boolean verified,
                boolean installedNow,
                String status,
                String installRoot,
                String treeSha256,
                String manifestSha256) {
            this.packaged = packaged;
            this.verified = verified;
            this.installedNow = installedNow;
            this.status = status;
            this.installRoot = installRoot;
            this.treeSha256 = treeSha256;
            this.manifestSha256 = manifestSha256;
        }

        static Result notPackaged() {
            return new Result(false, false, false, "not_packaged", "", "", "");
        }

        static Result reused(File root, BundleManifest manifest, String manifestSha) {
            return new Result(true, true, false, "verified_existing", root.getAbsolutePath(), manifest.treeSha256, manifestSha);
        }

        static Result installed(File root, BundleManifest manifest, String manifestSha) {
            return new Result(true, true, true, "installed_verified", root.getAbsolutePath(), manifest.treeSha256, manifestSha);
        }

        JSONObject toJson() throws Exception {
            JSONObject json = new JSONObject();
            json.put("schema", 1);
            json.put("component_id", EXPECTED_COMPONENT);
            json.put("packaged", packaged);
            json.put("verified", verified);
            json.put("installed_now", installedNow);
            json.put("status", status);
            json.put("install_root", installRoot);
            json.put("tree_sha256", treeSha256);
            json.put("manifest_sha256", manifestSha256);
            json.put("service_round_trip_proven", false);
            return json;
        }
    }

    private static final class Verification {
        final boolean valid;
        final String reason;
        private Verification(boolean valid, String reason) { this.valid = valid; this.reason = reason; }
        static Verification ok() { return new Verification(true, "ok"); }
        static Verification fail(String reason) { return new Verification(false, reason); }
    }

    private static final class ManifestFile {
        final String path;
        final long size;
        final String sha256;
        ManifestFile(String path, long size, String sha256) {
            this.path = path;
            this.size = size;
            this.sha256 = sha256;
        }
    }

    private static final class BundleManifest {
        final String treeSha256;
        final long totalBytes;
        final List<ManifestFile> files;

        private BundleManifest(String treeSha256, long totalBytes, List<ManifestFile> files) {
            this.treeSha256 = treeSha256;
            this.totalBytes = totalBytes;
            this.files = files;
        }

        static BundleManifest parse(byte[] bytes) throws Exception {
            JSONObject root = new JSONObject(new String(bytes, StandardCharsets.UTF_8));
            if (root.optInt("schema", -1) != 1
                    || !EXPECTED_COMPONENT.equals(root.optString("component_id"))
                    || !EXPECTED_VERSION.equals(root.optString("version"))
                    || !EXPECTED_PROFILE.equals(root.optString("profile_id"))
                    || !EXPECTED_NODE.equals(root.optString("required_node_version"))
                    || !EXPECTED_SOURCE_SHA256.equals(root.optString("source_archive_sha256"))
                    || !EXPECTED_PATCH_SHA256.equals(root.optString("routing_patch_profile_sha256"))) {
                throw new IOException("omniroute_bundle_identity_mismatch");
            }
            if (root.optBoolean("apk_asset_packaged", true)
                    || root.optBoolean("service_round_trip_proven", true)) {
                throw new IOException("omniroute_bundle_manifest_overclaims_proof");
            }
            JSONObject runtime = root.optJSONObject("runtime");
            if (runtime == null
                    || !"127.0.0.1".equals(runtime.optString("bind_host"))
                    || runtime.optInt("port", -1) != 20128
                    || !"server-ws.mjs".equals(runtime.optString("entrypoint"))) {
                throw new IOException("omniroute_bundle_runtime_contract_mismatch");
            }
            JSONObject runtimeEnvironment = runtime.optJSONObject("environment");
            if (runtimeEnvironment == null
                    || !"20128".equals(runtimeEnvironment.optString("PORT"))
                    || !"20128".equals(runtimeEnvironment.optString("DASHBOARD_PORT"))
                    || !"127.0.0.1".equals(runtimeEnvironment.optString("HOSTNAME"))
                    || !"true".equals(runtimeEnvironment.optString("VECTOR_STORE_DISABLE_VEC"))
                    || !"1".equals(runtimeEnvironment.optString("OMNIROUTE_MITM_STUB"))) {
                throw new IOException("omniroute_bundle_runtime_environment_mismatch");
            }
            String tree = root.optString("tree_sha256", "");
            if (!isSha256(tree)) throw new IOException("omniroute_bundle_tree_sha_invalid");
            long total = root.optLong("total_bytes", -1);
            int declaredCount = root.optInt("file_count", -1);
            if (total < 0 || total > MAX_TOTAL_BYTES || declaredCount < 1 || declaredCount > MAX_FILES) {
                throw new IOException("omniroute_bundle_limits_invalid");
            }
            JSONArray array = root.optJSONArray("files");
            if (array == null || array.length() != declaredCount) {
                throw new IOException("omniroute_bundle_file_count_mismatch");
            }
            List<ManifestFile> files = new ArrayList<>(declaredCount);
            Set<String> seen = new HashSet<>();
            long calculatedTotal = 0;
            MessageDigest treeDigest = MessageDigest.getInstance("SHA-256");
            for (int i = 0; i < array.length(); i++) {
                JSONObject item = array.getJSONObject(i);
                String path = item.optString("path", "");
                validateRelativePath(path);
                if (MANIFEST_NAME.equals(path) || RECEIPT_NAME.equals(path) || !seen.add(path)) {
                    throw new IOException("omniroute_bundle_duplicate_or_reserved_path:" + path);
                }
                long size = item.optLong("size", -1);
                String sha = item.optString("sha256", "");
                if (size < 0 || size > MAX_FILE_BYTES || !isSha256(sha)) {
                    throw new IOException("omniroute_bundle_file_metadata_invalid:" + path);
                }
                calculatedTotal = checkedAdd(calculatedTotal, size);
                if (calculatedTotal > MAX_TOTAL_BYTES) throw new IOException("omniroute_bundle_total_limit_exceeded");
                treeDigest.update(path.getBytes(StandardCharsets.UTF_8));
                treeDigest.update((byte) 0);
                treeDigest.update(Long.toString(size).getBytes(StandardCharsets.US_ASCII));
                treeDigest.update((byte) 0);
                treeDigest.update(sha.getBytes(StandardCharsets.US_ASCII));
                treeDigest.update((byte) '\n');
                files.add(new ManifestFile(path, size, sha));
            }
            if (calculatedTotal != total) throw new IOException("omniroute_bundle_declared_total_mismatch");
            if (!constantEquals(hex(treeDigest.digest()), tree)) throw new IOException("omniroute_bundle_tree_digest_mismatch");
            for (String required : new String[] {
                    "server.js",
                    "server-ws.mjs",
                    "package.json",
                    "build/runtime-env.mjs",
                    "build/bootstrap-env.mjs",
                    "healthcheck.mjs",
                    "node_modules/sql.js/package.json"
            }) {
                if (!seen.contains(required)) throw new IOException("omniroute_bundle_required_file_missing:" + required);
            }
            boolean migrationPresent = false;
            for (String path : seen) {
                if (path.startsWith("migrations/") && path.length() > "migrations/".length()) {
                    migrationPresent = true;
                    break;
                }
            }
            if (!migrationPresent) throw new IOException("omniroute_bundle_migrations_missing");
            return new BundleManifest(tree, total, Collections.unmodifiableList(files));
        }

        private static boolean isSha256(String value) {
            return value != null && value.matches("^[0-9a-f]{64}$");
        }
    }
}
