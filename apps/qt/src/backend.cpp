#include "backend.h"
#include <QDateTime>
#include <QFileInfo>
#include <QJsonDocument>
#include <cmath>
static constexpr int MaxFrame = 8 * 1024 * 1024;
Backend::Backend(QObject *parent) : QObject(parent) {
    connect(&process, &QProcess::readyReadStandardOutput, this, &Backend::receive);
    connect(&process, &QProcess::readyReadStandardError, this,
            [this] { process.readAllStandardError(); });
    connect(&process, &QProcess::started, this, [this] {
        request(QStringLiteral("hello"), {}, [this](const QJsonValue &v, const QJsonObject &e) {
            if (!e.isEmpty() ||
                v.toObject().value(QStringLiteral("protocolVersion")).toInt() != 1) {
                fail(tr("Несовместимая версия игрового ядра."));
                process.kill();
                return;
            }
            emit ready();
        });
    });
    connect(&process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError) {
        fail(tr("Не удалось запустить игровое ядро. Проверьте файлы установки."));
    });
    connect(&process, qOverload<int, QProcess::ExitStatus>(&QProcess::finished), this,
            [this](int, QProcess::ExitStatus) {
                fail(tr("Соединение с игровым ядром завершено. Перезапустите лаунчер."));
            });
    timer.setInterval(10000);
    connect(&timer, &QTimer::timeout, this, [this] {
        const auto now = QDateTime::currentMSecsSinceEpoch();
        QList<quint64> expired;
        for (auto it = pending.cbegin(); it != pending.cend(); ++it)
            if (now - it.value().created > 30 * 60 * 1000)
                expired.append(it.key());
        // Callbacks may immediately enqueue another request or clear all pending
        // requests after a disconnect. Never keep a QHash iterator across them.
        for (const auto id : expired) {
            if (pending.contains(id)) {
                const auto cb = pending.take(id).reply;
                if (cb)
                    cb({},
                       {{QStringLiteral("message"), tr("Операция заняла слишком много времени. "
                                                       "Проверьте её состояние перед повтором.")}});
            }
        }
    });
    timer.start();
}
void Backend::start(const QString &executable, const QProcessEnvironment &environment) {
    process.setProcessEnvironment(environment);
    process.setProgram(QFileInfo(executable).absoluteFilePath());
    process.setProcessChannelMode(QProcess::SeparateChannels);
    process.start();
}
quint64 Backend::request(const QString &method, const QJsonObject &params, Reply reply) {
    const quint64 id = nextId++;
    const bool control = method == QStringLiteral("stop_game") ||
                         method == QStringLiteral("cancel_operation") ||
                         method == QStringLiteral("cancel_content_operation") ||
                         method == QStringLiteral("cancel_microsoft_login") ||
                         method == QStringLiteral("launch_status") ||
                         method == QStringLiteral("installation_status");
    // Read/download requests must not consume the last slots needed to stop them.
    if (process.state() != QProcess::Running || pending.size() >= (control ? 72 : 64)) {
        if (reply)
            QTimer::singleShot(0, this, [reply] {
                reply({}, {{QStringLiteral("message"),
                            tr("Ядро пока недоступно. Повторите после подключения.")}});
            });
        return id;
    }
    const QJsonObject request{{QStringLiteral("id"), static_cast<double>(id)},
                              {QStringLiteral("method"), method},
                              {QStringLiteral("params"), params}};
    const QByteArray bytes = QJsonDocument(request).toJson(QJsonDocument::Compact) + '\n';
    if (bytes.size() > 1024 * 1024) {
        if (reply)
            reply({}, {{QStringLiteral("message"), tr("Слишком большой запрос.")}});
        return id;
    }
    pending.insert(id, {std::move(reply), QDateTime::currentMSecsSinceEpoch()});
    if (process.write(bytes) < 0)
        fail(tr("Не удалось передать запрос игровому ядру."));
    return id;
}
bool Backend::decode(const QByteArray &line, QJsonObject *result) {
    if (line.size() > MaxFrame)
        return false;
    QJsonParseError error;
    const auto document = QJsonDocument::fromJson(line, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return false;
    const auto object = document.object();
    if (object.contains(QStringLiteral("event"))) {
        if (!object.value(QStringLiteral("event")).isString() ||
            !object.contains(QStringLiteral("data")))
            return false;
    } else {
        const auto id = object.value(QStringLiteral("id")).toDouble(-1);
        if (!object.value(QStringLiteral("id")).isDouble() || id < 1 || id > 9007199254740991.0 ||
            std::floor(id) != id ||
            object.contains(QStringLiteral("result")) == object.contains(QStringLiteral("error")))
            return false;
        if (object.contains(QStringLiteral("error")) &&
            (!object.value(QStringLiteral("error")).isObject() ||
             object.value(QStringLiteral("error")).toObject().isEmpty()))
            return false;
    }
    *result = object;
    return true;
}
void Backend::receive() {
    buffer += process.readAllStandardOutput();
    while (true) {
        const int newline = buffer.indexOf('\n');
        if (newline < 0)
            break;
        QJsonObject message;
        if (!decode(buffer.left(newline), &message)) {
            fail(tr("Повреждён ответ игрового ядра."));
            process.kill();
            return;
        }
        buffer.remove(0, newline + 1);
        if (message.contains(QStringLiteral("event"))) {
            emit event(message.value(QStringLiteral("event")).toString(),
                       message.value(QStringLiteral("data")));
            continue;
        }
        const auto id = static_cast<quint64>(message.value(QStringLiteral("id")).toDouble());
        if (!pending.contains(id))
            continue;
        const auto item = pending.take(id);
        if (item.reply)
            item.reply(message.value(QStringLiteral("result")),
                       message.value(QStringLiteral("error")).toObject());
    }
    if (buffer.size() > MaxFrame) {
        fail(tr("Ответ ядра превышает допустимый размер."));
        process.kill();
    }
}
void Backend::fail(const QString &message) {
    const auto callbacks = pending;
    pending.clear();
    buffer.clear();
    for (const auto &item : callbacks)
        if (item.reply)
            item.reply({}, {{QStringLiteral("message"), message}});
    emit disconnected(message);
}
void Backend::shutdown() {
    process.closeWriteChannel();
    if (!process.waitForFinished(1000)) {
        process.terminate();
        if (!process.waitForFinished(500))
            process.kill();
    }
}
