#include "emote.h"
#include <QEasingCurve>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QQuaternion>
#include <algorithm>
#include <cmath>
namespace {
constexpr double pi = 3.14159265358979323846;
QString text(const char *value) {
    return QString::fromLatin1(value);
}
bool boolean(const QJsonValue &value, bool fallback = false) {
    return value.isString() ? value.toString().compare(text("true"), Qt::CaseInsensitive) == 0
                            : value.toBool(fallback);
}
QQuaternion rotation(const QVector3D &r, float weight) {
    // ModelPart uses Z-Y-X Euler order. Blend orientations, not Euler components:
    // fading a pose across +/- pi must not send an arm through an extra full turn.
    const auto q = QQuaternion::fromAxisAndAngle(0, 0, 1, r.z() * 180 / pi) *
                   QQuaternion::fromAxisAndAngle(0, 1, 0, r.y() * 180 / pi) *
                   QQuaternion::fromAxisAndAngle(1, 0, 0, r.x() * 180 / pi);
    return QQuaternion::slerp(QQuaternion(), q, qBound(0.f, weight, 1.f));
}
QVector3D bendAxis(float axis, bool upper) {
    // Minecraft model coordinates -> our Y-up, front-facing render coordinates.
    return {std::cos(axis), 0, (upper ? 1.f : -1.f) * std::sin(axis)};
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
    if (!std::isfinite(endTick) || endTick <= 0)
        return;
    beginTick = qBound(0.0, emote.value(text("beginTick")).toDouble(), endTick);
    stopTick = qMax(endTick, emote.value(text("stopTick")).toDouble(endTick));
    returnTick = qBound(0.0, emote.value(text("returnTick")).toDouble(), endTick);
    loop = boolean(emote.value(text("isLoop")));
    easingBefore = boolean(emote.value(text("easeBeforeKeyframe")));
    const bool degrees = boolean(emote.value(text("degrees")), true);
    const int version = source.value(text("version")).toInt(1);
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
            // Emotecraft v1/v2 calls the root "torso"; v3 introduced a separate
            // chest bone. See PlayerAnimator's AnimationJson (MIT notice bundled).
            if (version < 3 && part == text("torso"))
                part = text("body");
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
                if (angle && degrees)
                    value *= pi / 180;
                const auto easing = move.value(text("easing")).toString(text("LINEAR")).toUpper();
                tracks[part][key].append({tick, value, easing});
                const auto turn = move.value(text("turn")).toDouble();
                if (angle && turn != 0 && std::isfinite(turn))
                    tracks[part][key].append({tick, value + turn * 2 * pi, easing});
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
double EmoteClip::channel(const QString &part, const QString &axis, double tick, double initial,
                          bool loopStarted) const {
    const auto p = tracks.constFind(part);
    if (p == tracks.cend())
        return initial;
    const auto frames = p->constFind(axis);
    if (frames == p->cend())
        return initial;
    if (!loop && tick > endTick) {
        if (tick >= stopTick || stopTick <= endTick)
            return initial;
        const auto end = channel(part, axis, endTick, initial, false);
        return end + (initial - end) * (tick - endTick) / (stopTick - endTick);
    }
    Frame previous{beginTick, initial, text("LINEAR")};
    int index = 0;
    while (index < frames->size() && (*frames)[index].tick <= tick)
        previous = (*frames)[index++];
    const double period = endTick - returnTick + 1;
    if (loopStarted && previous.tick < returnTick) {
        previous = frames->last();
        previous.tick -= period;
    }
    Frame next{endTick, previous.value, previous.easing};
    if (index < frames->size())
        next = (*frames)[index];
    else if (loop) {
        next = {returnTick + period, initial, text("LINEAR")};
        for (const auto &candidate : *frames)
            if (candidate.tick >= returnTick) {
                next = candidate;
                next.tick += period;
                break;
            }
    }
    if (next.tick <= previous.tick)
        return previous.value;
    const auto progress = ease(easingBefore ? next.easing : previous.easing,
                               (tick - previous.tick) / (next.tick - previous.tick));
    return previous.value + (next.value - previous.value) * progress;
}
EmotePart EmoteClip::sample(const QString &part, double tick, const QVector3D &bind) const {
    tick = std::isfinite(tick) ? qMax(0.0, tick) : 0;
    const bool loopStarted = loop && tick >= endTick + 1;
    if (loopStarted)
        tick = returnTick + std::fmod(tick - returnTick, endTick - returnTick + 1);
    const auto c = [&](const char *key, double initial = 0) {
        return float(channel(part, text(key), tick, initial, loopStarted));
    };
    return {{c("x", bind.x()), c("y", bind.y()), c("z", bind.z())},
            {c("pitch"), c("yaw"), c("roll")},
            {c("scaleX", 1), c("scaleY", 1), c("scaleZ", 1)},
            c("bend"),
            c("axis")};
}
QMatrix4x4 EmoteClip::modelTransform(const EmotePart &part, float weight) {
    QMatrix4x4 m;
    m.translate(part.position.x(), 24 - part.position.y(), -part.position.z());
    m.rotate(rotation({part.rotation.x(), -part.rotation.y(), -part.rotation.z()}, weight));
    m.scale(part.scale);
    return m;
}
QMatrix4x4 EmoteClip::bodyTransform(const EmotePart &part, float weight) {
    QMatrix4x4 m;
    // Root motion is in blocks, before Minecraft's model-coordinate reflection.
    // Its pivot is 0.7 blocks above the feet, NOT the chest/neck model origin.
    m.scale(part.scale);
    m.translate(-16 * part.position.x(), 16 * part.position.y() + 11.2f, -16 * part.position.z());
    m.rotate(rotation({-part.rotation.x(), part.rotation.y(), -part.rotation.z()}, weight));
    m.translate(0, -11.2f, 0);
    return m;
}
QMatrix4x4 EmoteClip::bendTransform(const QVector3D &center, float bend, float axis, bool upper) {
    QMatrix4x4 turn;
    turn.translate(center);
    turn.rotate(float(bend * 180 / pi), bendAxis(axis, upper));
    turn.translate(-center);
    return turn;
}
QVector3D EmoteClip::bendVertex(QVector3D point, const QVector3D &center, float halfLength,
                                float bend, float axis, bool upper) {
    if (std::abs(bend) < .00001f || halfLength <= 0)
        return point;
    // BendyLib's two rigid halves meet at a shared, sheared joint plane. Rotating
    // each vertex by a different angle instead shrinks/twists elbows and knees.
    // Use the actual cuboid center, including the off-center classic/slim arms.
    // Adapted from KosmX/BendyLib IBendable (MIT; licenses/animation-reference-MIT.txt).
    bend = qBound(float(-pi + .0001), float(std::remainder(bend, 2 * pi)), float(pi - .0001));
    const QVector3D direction(0, upper ? 1.f : -1.f, 0);
    auto relative = point - center;
    const auto distance = QVector3D::dotProduct(relative, direction);
    const auto across =
        QVector3D::dotProduct(QVector3D::crossProduct(direction, bendAxis(axis, upper)), relative);
    const auto end = distance > 0 ? halfLength : -halfLength;
    relative += direction * ((distance - end) / halfLength * std::tan(bend / 2) * across);
    if (distance > 0) {
        const auto turn = QQuaternion::fromAxisAndAngle(bendAxis(axis, upper), bend * 180 / pi);
        relative = turn.rotatedVector(relative);
    }
    return relative + center;
}
