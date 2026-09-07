#include "backend.h"
#include <QDateTime>
#include <QFileInfo>
#include <QJsonDocument>
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
        for (auto it = pending.begin(); it != pending.end();) {
            if (now - it.value().created > 30 * 60 * 1000) {
                auto cb = it.value().reply;
                it = pending.erase(it);
                if (cb)
                    cb({},
                       {{QStringLiteral("message"), tr("Операция заняла слишком много времени. "
                                                       "Проверьте её состояние перед повтором.")}});
            } else
                ++it;
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
    if (process.state() != QProcess::Running || pending.size() >= 64) {
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
    } else if (!object.value(QStringLiteral("id")).isDouble() ||
               object.contains(QStringLiteral("result")) ==
                   object.contains(QStringLiteral("error")))
        return false;
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
