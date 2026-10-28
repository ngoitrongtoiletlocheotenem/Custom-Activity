// Current official Discord Social SDK only. No private IPC protocol or tokens.
#define DISCORDPP_IMPLEMENTATION
#include "discordpp.h"
#include <chrono>
#include <memory>
#include <thread>
#include <cstring>
#include <string>

namespace {
struct State { bool connected = false;
#ifdef CA_DISCORD_DIAGNOSTICS
    bool assets_created = false, set_assets_called = false, assets_attached = false;
    bool callback_received = false, callback_successful = false;
    int update_result = -1;
    int32_t callback_error_code = 0;
    std::string large_image, large_text;
#endif
};
#ifdef CA_DISCORD_DIAGNOSTICS
std::string json_string(const std::string& value) {
    std::string result = "\"";
    constexpr char hex[] = "0123456789abcdef";
    for (unsigned char c : value) {
        if (c == '"' || c == '\\') { result += '\\'; result += static_cast<char>(c); }
        else if (c < 0x20) { result += "\\u00"; result += hex[c >> 4]; result += hex[c & 15]; }
        else result += static_cast<char>(c);
    }
    return result + '"';
}
#endif
struct Bridge {
    std::unique_ptr<discordpp::Client> client;
    std::shared_ptr<State> state = std::make_shared<State>();
    uint64_t id;
    std::chrono::steady_clock::time_point last_probe = std::chrono::steady_clock::now();
    explicit Bridge(uint64_t application_id) : client(std::make_unique<discordpp::Client>()), id(application_id) {
        client->SetApplicationId(id);
    }
};
struct Completion { bool done = false; bool success = false; };
int wait(const std::shared_ptr<Completion>& completion) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    while (!completion->done && std::chrono::steady_clock::now() < deadline) {
        discordpp::RunCallbacks();
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    return !completion->done ? 2 : completion->success ? 0 : 1;
}
}

extern "C" {
void* ca_discord_create(uint64_t id) noexcept {
    try { return new Bridge(id); } catch (...) { return nullptr; }
}
void ca_discord_destroy(void* handle) noexcept {
    try { delete static_cast<Bridge*>(handle); } catch (...) {}
}
int ca_discord_connect(void* handle) noexcept {
    try {
        auto& bridge = *static_cast<Bridge*>(handle);
        bridge.last_probe = std::chrono::steady_clock::now();
        auto completion = std::make_shared<Completion>();
        // Only checks local SDK connectivity. No user identifier is retained,
        // serialized, logged, or returned through the ABI.
        auto state = bridge.state;
        bridge.client->GetDiscordClientConnectedUser(bridge.id, [completion, state](auto result, auto user) {
            completion->success = result.Successful() && user.has_value();
            completion->done = true;
            state->connected = completion->success;
        });
        int code = wait(completion);
        if (code != 0) state->connected = false;
        return code;
    } catch (...) { return 1; }
}
int ca_discord_connected(void* handle) noexcept {
    return handle && static_cast<Bridge*>(handle)->state->connected ? 1 : 0;
}
int ca_discord_set(void* handle, const char* name, const char* details, const char* image, const char* image_text) noexcept {
    try {
        auto& bridge = *static_cast<Bridge*>(handle);
#ifdef CA_DISCORD_DIAGNOSTICS
        bridge.state->assets_created = false;
        bridge.state->set_assets_called = false;
        bridge.state->assets_attached = false;
        bridge.state->callback_received = false;
        bridge.state->callback_successful = false;
        bridge.state->update_result = -1;
        bridge.state->callback_error_code = 0;
        bridge.state->large_image.clear();
        bridge.state->large_text.clear();
#endif
        discordpp::Activity activity;
        activity.SetType(discordpp::ActivityTypes::Playing);
        activity.SetName(name);
        if (*details) activity.SetDetails(details);
        if (*image) {
            discordpp::ActivityAssets assets;
#ifdef CA_DISCORD_DIAGNOSTICS
            bridge.state->assets_created = static_cast<bool>(assets);
#endif
            assets.SetLargeImage(image);
            if (*image_text) assets.SetLargeText(image_text);
            activity.SetAssets(assets);
#ifdef CA_DISCORD_DIAGNOSTICS
            bridge.state->set_assets_called = true;
#endif
        }
#ifdef CA_DISCORD_DIAGNOSTICS
        // Read back from the actual activity after the local assets object and
        // optional<string> setter temporaries have been destroyed. This also
        // verifies the SDK's owned-copy contract across SetAssets.
        auto attached_assets = activity.Assets();
        bridge.state->assets_attached = attached_assets.has_value();
        if (attached_assets) {
            bridge.state->large_image = attached_assets->LargeImage().value_or("");
            bridge.state->large_text = attached_assets->LargeText().value_or("");
        }
#endif
        auto completion = std::make_shared<Completion>();
        auto state = bridge.state;
        bridge.client->UpdateRichPresence(activity, [completion, state](auto result) {
#ifdef CA_DISCORD_DIAGNOSTICS
            state->callback_received = true;
            state->callback_successful = result.Successful();
            state->callback_error_code = result.ErrorCode();
#endif
            completion->success = result.Successful();
            completion->done = true;
            state->connected = completion->success;
        });
        int code = wait(completion);
#ifdef CA_DISCORD_DIAGNOSTICS
        bridge.state->update_result = code;
#endif
        if (code != 0) state->connected = false;
        return code;
    } catch (...) { return 1; }
}
// Copy-out ABI: no pointer to C++ temporary memory crosses the boundary.
// Required size includes NUL. No description, token or user identity is included.
size_t ca_discord_diagnostics(void* handle, char* output, size_t capacity) noexcept {
#ifdef CA_DISCORD_DIAGNOSTICS
    try {
        if (!handle) return 0;
        const auto& s = *static_cast<Bridge*>(handle)->state;
        const auto boolean = [](bool v) { return v ? "true" : "false"; };
        std::string json = std::string("{\"assetsCreated\":") + boolean(s.assets_created)
            + ",\"setAssetsCalled\":" + boolean(s.set_assets_called)
            + ",\"assetsAttached\":" + boolean(s.assets_attached)
            + ",\"finalLargeImage\":" + json_string(s.large_image)
            + ",\"finalLargeText\":" + json_string(s.large_text)
            + ",\"updateCallbackReceived\":" + boolean(s.callback_received)
            + ",\"updateCallbackSuccessful\":" + boolean(s.callback_successful)
            + ",\"updateCallbackErrorCode\":" + std::to_string(s.callback_error_code)
            + ",\"updateResult\":" + std::to_string(s.update_result) + "}";
        if (output && capacity >= json.size() + 1) std::memcpy(output, json.c_str(), json.size() + 1);
        return json.size() + 1;
    } catch (...) { return 0; }
#else
    (void)handle; (void)output; (void)capacity;
    return 0;
#endif
}
int ca_discord_clear(void* handle) noexcept {
    try {
        static_cast<Bridge*>(handle)->client->ClearRichPresence();
        // ClearRichPresence has no completion callback in the official API.
        // Give the desktop transport a bounded chance to flush before exit.
        for (int i = 0; i < 10; ++i) {
            discordpp::RunCallbacks();
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        return 0;
    } catch (...) { return 1; }
}
int ca_discord_tick(void* handle) noexcept {
    try {
        discordpp::RunCallbacks();
        auto& bridge = *static_cast<Bridge*>(handle);
        // A desktop process can survive an IPC disconnect. Probe the official
        // transport without republishing an unchanged activity or busy-looping.
        if (std::chrono::steady_clock::now() - bridge.last_probe >= std::chrono::seconds(15)) {
            return ca_discord_connect(handle);
        }
        return 0;
    } catch (...) { return 1; }
}
}
