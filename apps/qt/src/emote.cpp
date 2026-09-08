#include "emote.h"
#include <QEasingCurve>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <algorithm>
#include <cmath>
namespace {
constexpr double pi = 3.14159265358979323846;
QString text(const char *value) {
    return QString::fromLatin1(value);
}
double ease(const QString &name, double p) {
    if (name == text("CONSTANT"))
        return p >= 1 ? 1 : 0;
    static const QMap<QString, QEasingCurve::Type> curves{
        {text("EASEINSINE"), QEasingCurve::InSine},
        {text("EASEOUTSINE"), QEasingCurve::OutSine},
        {text("EASEINOUTSINE"), QEasingCurve::InOutSine},
        {text("EASEINQUAD"), QEasingCurve::InQuad},
        {text("EASEOUTQUAD"), QEasingCurve::OutQuad},
        {text("EASEINOUTQUAD"), QEasingCurve::InOutQuad},
        {text("EASEINCUBIC"), QEasingCurve::InCubic},
        {text("EASEOUTCUBIC"), QEasingCurve::OutCubic},
        {text("EASEINOUTCUBIC"), QEasingCurve::InOutCubic},
        {text("EASEINQUART"), QEasingCurve::InQuart},
        {text("EASEOUTQUART"), QEasingCurve::OutQuart},
        {text("EASEINOUTQUART"), QEasingCurve::InOutQuart},
        {text("EASEINQUINT"), QEasingCurve::InQuint},
        {text("EASEOUTQUINT"), QEasingCurve::OutQuint},
        {text("EASEINOUTQUINT"), QEasingCurve::InOutQuint},
        {text("EASEINEXPO"), QEasingCurve::InExpo},
        {text("EASEOUTEXPO"), QEasingCurve::OutExpo},
        {text("EASEINOUTEXPO"), QEasingCurve::InOutExpo},
        {text("EASEINCIRC"), QEasingCurve::InCirc},
        {text("EASEOUTCIRC"), QEasingCurve::OutCirc},
        {text("EASEINOUTCIRC"), QEasingCurve::InOutCirc},
        {text("EASEINBACK"), QEasingCurve::InBack},
        {text("EASEOUTBACK"), QEasingCurve::OutBack},
        {text("EASEINOUTBACK"), QEasingCurve::InOutBack},
        {text("EASEINELASTIC"), QEasingCurve::InElastic},
        {text("EASEOUTELASTIC"), QEasingCurve::OutElastic},
        {text("EASEINOUTELASTIC"), QEasingCurve::InOutElastic},
        {text("EASEINBOUNCE"), QEasingCurve::InBounce},
        {text("EASEOUTBOUNCE"), QEasingCurve::OutBounce},
        {text("EASEINOUTBOUNCE"), QEasingCurve::InOutBounce}};
    return QEasingCurve(curves.value(name, QEasingCurve::Linear))
        .valueForProgress(qBound(0.0, p, 1.0));
}
} // namespace
EmoteClip::EmoteClip(const QJsonObject &source) {
    const auto emote = source.value(text("emote")).toObject();
    endTick = emote.value(text("endTick")).toDouble();
    easingBefore = emote.value(text("isEasingBefore")).toBool(true);
    const bool degrees = emote.value(text("degrees")).toBool();
    for (const auto &entry : emote.value(text("moves")).toArray()) {
        const auto move = entry.toObject();
        const auto tick = move.value(text("tick")).toDouble();
        if (!std::isfinite(tick) || tick < 0)
            continue;
        for (auto it = move.begin(); it != move.end(); ++it) {
            if (!it.value().isObject())
                continue;
            auto part = it.key();
            part.replace(text("right_arm"), text("rightArm"));
            part.replace(text("left_arm"), text("leftArm"));
            part.replace(text("right_leg"), text("rightLeg"));
            part.replace(text("left_leg"), text("leftLeg"));
            const auto channels = it.value().toObject();
            for (auto c = channels.begin(); c != channels.end(); ++c) {
                if (!c.value().isDouble() || !std::isfinite(c.value().toDouble()))
                    continue;
                auto key = c.key();
                if (key == text("bendDirection"))
                    key = text("axis");
                double value = c.value().toDouble();
                const bool angle = key == text("pitch") || key == text("yaw") ||
                                   key == text("roll") || key == text("bend") ||
                                   key == text("axis");
                if (angle)
                    value = (degrees ? value * pi / 180 : value) +
                            move.value(text("turn")).toDouble() * 2 * pi;
                tracks[part][key].append(
                    {tick, value, move.value(text("easing")).toString(text("LINEAR")).toUpper()});
            }
        }
    }
    for (auto &part : tracks)
        for (auto &track : part)
            std::stable_sort(track.begin(), track.end(),
                             [](const Frame &a, const Frame &b) { return a.tick < b.tick; });
}
EmoteClip EmoteClip::load(const QString &resource) {
    QFile file(resource);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    return EmoteClip(QJsonDocument::fromJson(file.readAll()).object());
}
double EmoteClip::channel(const QString &part, const QString &axis, double tick,
                          double initial) const {
    const auto p = tracks.constFind(part);
    if (p == tracks.cend())
        return initial;
    const auto frames = p->constFind(axis);
    if (frames == p->cend())
        return initial;
    Frame previous{0, initial, text("LINEAR")};
    for (const auto &next : *frames) {
        if (tick < next.tick) {
            if (next.tick <= previous.tick)
                return next.value;
            const auto progress = ease(easingBefore ? next.easing : previous.easing,
                                       (tick - previous.tick) / (next.tick - previous.tick));
            return previous.value + (next.value - previous.value) * progress;
        }
        previous = next;
    }
    return previous.value;
}
EmotePart EmoteClip::sample(const QString &part, double tick, const QVector3D &bind) const {
    const auto c = [&](const char *key, double initial = 0) {
        return float(channel(part, text(key), tick, initial));
    };
    return {{c("x", bind.x()), c("y", bind.y()), c("z", bind.z())},
            {c("pitch"), c("yaw"), c("roll")},
            {c("scaleX", 1), c("scaleY", 1), c("scaleZ", 1)},
            c("bend"),
            c("axis")};
}
QVector3D EmoteClip::bendVertex(QVector3D point, float pivot, float bend, float axis, bool upper) {
    if (std::abs(bend) < .00001f)
        return point;
    // A shared joint ring rotates by half the angle. Both halves meet at exactly
    // the same vertices; UVs and outer clothing layers remain continuous.
    const auto distance = upper ? point.y() - pivot : pivot - point.y();
    const auto weight = qBound(0.f, .5f + distance / 2.f, 1.f);
    QMatrix4x4 turn;
    turn.translate(0, pivot, 0);
    turn.rotate(float(bend * 180 / pi * weight), std::cos(axis), 0, std::sin(axis));
    turn.translate(0, -pivot, 0);
    return turn.map(point);
}
