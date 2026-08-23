import { useEffect, useState } from "react";

import backgroundOne from "../assets/bg-1.jpg";
import backgroundTwo from "../assets/bg-2.jpg";
import backgroundThree from "../assets/bg-3.jpg";

const backgrounds = [backgroundOne, backgroundTwo, backgroundThree];

export function BackgroundCarousel() {
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    if (reducedMotion) return;
    const timer = window.setInterval(() => {
      setActiveIndex((current) => (current + 1) % backgrounds.length);
    }, 12_000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div aria-label="Фоновый кадр" className="background-carousel">
      {backgrounds.map((background, index) => (
        <img
          alt=""
          aria-hidden="true"
          className={index === activeIndex ? "is-active" : ""}
          key={background}
          src={background}
        />
      ))}
      <div className="carousel-dots">
        {backgrounds.map((background, index) => (
          <button
            aria-label={`Кадр ${index + 1}`}
            aria-pressed={index === activeIndex}
            key={background}
            onClick={() => setActiveIndex(index)}
            type="button"
          />
        ))}
      </div>
    </div>
  );
}
