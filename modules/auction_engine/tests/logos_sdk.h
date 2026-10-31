#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <nlohmann/json.hpp>

#include "lez_core_api.h"

namespace lez_test_hooks {
// Makes the next send report a transport failure (a CallError), which the CMock store cannot express.
inline bool failNextSend = false;
}  // namespace lez_test_hooks

// Test adapter between the universal module's std-based generated API and the
// Qt test framework's generated dependency client.
class UniversalLezCore {
public:
    explicit UniversalLezCore(LogosAPI* api)
        : qt_(api) { }

    std::int64_t open(const std::string& config,
                      const std::string& storage,
                      const std::string& statistics,
                      logos::CallError* error = nullptr) {
        return qt_.open(QString::fromStdString(config),
                        QString::fromStdString(storage),
                        QString::fromStdString(statistics),
                        error);
    }

    nlohmann::json list_accounts(logos::CallError* error = nullptr) {
        const QVariantList accounts = qt_.list_accounts(error);
        const QByteArray text =
            QJsonDocument(QJsonArray::fromVariantList(accounts)).toJson(QJsonDocument::Compact);
        return nlohmann::json::parse(text.toStdString());
    }

    std::int64_t get_current_block_height(logos::CallError* error = nullptr) {
        return qt_.get_current_block_height(error);
    }

    std::string send_generic_public_transaction(
        const std::vector<std::string>& account_ids,
        const std::vector<bool>& signing_requirements,
        const std::vector<std::uint8_t>& instruction,
        const std::string& program_id,
        logos::CallError* error = nullptr) {
        QStringList qt_account_ids;
        qt_account_ids.reserve(static_cast<qsizetype>(account_ids.size()));
        for (const auto& account_id : account_ids) {
            qt_account_ids.push_back(QString::fromStdString(account_id));
        }

        QVariantList qt_signing_requirements;
        qt_signing_requirements.reserve(static_cast<qsizetype>(signing_requirements.size()));
        for (const bool required : signing_requirements) {
            qt_signing_requirements.push_back(required);
        }

        if (lez_test_hooks::failNextSend) {
            lez_test_hooks::failNextSend = false;
            if (error != nullptr) {
                error->code = "object_unavailable";
                error->message = "test: lez_core did not answer";
            }
            return {};
        }

        const QByteArray qt_instruction(reinterpret_cast<const char*>(instruction.data()),
                                        static_cast<qsizetype>(instruction.size()));
        return qt_.send_generic_public_transaction(qt_account_ids,
                                                   qt_signing_requirements,
                                                   QVariant(qt_instruction),
                                                   QString::fromStdString(program_id),
                                                   error)
            .toStdString();
    }

private:
    LezCore qt_;
};

struct LogosModules {
    explicit LogosModules(LogosAPI* api)
        : lez_core(api) { }

    UniversalLezCore lez_core;
};
